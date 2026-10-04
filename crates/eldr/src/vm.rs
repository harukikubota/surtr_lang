mod test_runner;
use test_runner::VmTestRunner;
pub use test_runner::{VmTestCase, VmTestDeclaration, VmTestPolicy, VmTestScope, VmTestScopeKind};
mod process_continuation;
pub(crate) use process_continuation::RuntimeContinuation;
use process_continuation::{DetachedTask, SingletonFlight};
use sindr::builtin::{builtin_meta_by_id, match_result_variant_meta};
use sindr::ir::{
    line_column_for_offset, validate_chunk_function_table, validate_program_function_table,
    validate_type_registry_append_entries, Bytecode, BytecodeChunk, CallableTemplate,
    CallableTemplateArg, CallableTemplateComposeFlavor, CallableTemplateDirectTarget,
    CallableTemplateKind, Constant, DocEntry, FunctionEntry, Opcode, RuntimeHandlerTarget,
    RuntimeInitPolicy, RuntimeProcessInstance, RuntimeProcessSpec, RuntimeProcessSpecTable,
    RuntimeSupervisorPolicy, SourceMap,
};
use sindr::names::{compiler_global_error_kind, IMPLICIT_ROOT_NAMESPACE_PREFIX};
use sindr::primitives::{int, SurtrInt, ToPrimitive, Zero};
use sindr::runtime::{
    Callable, CallableMetadata, CallableOrigin, CallableTarget, FileHandleValue, ListHandle,
    Location, PidHandle, RichError, RuntimeCallKind, RuntimeExecutionPhase,
    RuntimeProcessTraceContext, RuntimeStackFrame, TypeRegistry, Value, WorkerLeaseHandle,
    WorkersHandle,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::rc::Rc;

use crate::builtin::{call_builtin, BuiltinOutcome};
mod continuation;
#[cfg(test)]
mod generator_tests;
#[cfg(test)]
mod list_flat_map_tests;
use crate::dbg_display::{render_dbg_report, DbgRenderArg};
use crate::error::{RuntimeError, RuntimeErrorContext};
use continuation::{ContinuationFrame, Invocation};
use std::env;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MAX_TAIL_CALL_TRACE_BREADCRUMBS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmTestEventKind {
    Passed,
    Failed,
    Skipped,
    Pending,
    Filtered,
    Runnable,
    ScopeFailed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmCapturedIo {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeOutputEvent {
    StdOut(String),
    StdErr(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmRuntimeOutputEventSnapshot {
    pub stream: String,
    pub text: String,
}

impl RuntimeOutputEvent {
    fn snapshot(&self) -> VmRuntimeOutputEventSnapshot {
        match self {
            Self::StdOut(text) => VmRuntimeOutputEventSnapshot {
                stream: "stdout".into(),
                text: text.clone(),
            },
            Self::StdErr(text) => VmRuntimeOutputEventSnapshot {
                stream: "stderr".into(),
                text: text.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmTestEvent {
    pub case: Option<VmTestCase>,
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
    pub assertion: Option<String>,
    pub assertion_call_kind: Option<RuntimeCallKind>,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub span_start: u32,
    pub span_end: u32,
}

impl VmTestDiagnostic {
    fn from_rich_error(error: &RichError, bytecode: &Bytecode) -> Self {
        // Use the saved failure trace, including tail calls. Public Test wrappers
        // may call another assertion; the outer assertion is the user's call.
        let assertion = if error.kind == compiler_global_error_kind("TestAssertionFailed") {
            error
                .stack_trace
                .iter()
                .filter_map(|frame| {
                    let entry = bytecode.functions.get(frame.fun_idx? as usize)?;
                    let name = sindr::names::surface_path_name(entry.qualified_name.as_deref()?)
                        .strip_prefix("Test::")?;
                    matches!(
                        name,
                        "assert_true"
                            | "assert_false"
                            | "assert_eq"
                            | "assert_ok_eq"
                            | "assert_err_contains"
                            | "assert_doc_plain_eq"
                            | "assert_doc_ansi_eq"
                            | "assert_stdout_eq"
                            | "assert_stderr_eq"
                    )
                    .then_some((name, frame))
                    .filter(|(_, frame)| frame.location.is_some())
                })
                .last()
        } else {
            None
        };
        let location = assertion
            .and_then(|(_, frame)| frame.location.as_ref())
            .unwrap_or(&error.location);
        Self {
            kind: error.kind.clone(),
            message: error.visible_message().to_string(),
            assertion: assertion.map(|(name, _)| name.to_string()),
            assertion_call_kind: assertion.map(|(_, frame)| frame.call_kind.clone()),
            file: location.file.clone(),
            line: location.line,
            column: location.column,
            span_start: location.span_start,
            span_end: location.span_end,
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
    trace_frame: Option<RuntimeStackFrame>,
    tail_call_breadcrumb_base_len: usize,
    locals: Vec<Value>,
}

#[derive(Debug, Clone)]
struct VmCheckpoint {
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    tail_call_breadcrumbs: VecDeque<RuntimeStackFrame>,
    continuations: Vec<ContinuationFrame>,
    pending_invocation: Option<Result<Invocation, RuntimeError>>,
    cancellation: Option<RuntimeError>,
    pc: usize,
    exit_code: i32,
    last_result: Option<Value>,
    output_len: Option<usize>,
    error_output_len: Option<usize>,
    test_runner: VmTestRunner,
    test_event_len: usize,
    test_stdout_cursor: usize,
    test_stderr_cursor: usize,
    stdin_input_cursor: usize,
    process_runtime: ProcessRuntime,
    opcode_len: usize,
    constant_len: usize,
    type_entry_len: usize,
    error_template_len: usize,
    callable_template_len: usize,
    function_len: usize,
    doc_len: usize,
    process_spec_len: usize,
    source_map_len: Option<usize>,
    overwritten_functions: Vec<(usize, FunctionEntry)>,
    open_files: HashMap<u64, VmOpenFile>,
    batch_shutdown_pending: bool,
    next_file_handle_id: u64,
    cwd: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VmFileMode {
    Read,
    Write,
    Append,
    ReadWrite,
    ReadAppend,
}

impl VmFileMode {
    fn can_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite | Self::ReadAppend)
    }

    fn can_write(self) -> bool {
        matches!(
            self,
            Self::Write | Self::Append | Self::ReadWrite | Self::ReadAppend
        )
    }
}

#[derive(Debug)]
struct VmOpenFile {
    path: String,
    mode: VmFileMode,
    // Checkpoints retain a lease without reopening the path or rewinding external I/O.
    file: Rc<RefCell<File>>,
}

impl Clone for VmOpenFile {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            mode: self.mode,
            file: Rc::clone(&self.file),
        }
    }
}

#[derive(Debug)]
pub(crate) enum VmFileError {
    Closed,
    Io(io::Error),
    Encoding(String),
    Message(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmObservationOptions {
    pub trace_opcodes: bool,
    pub trace_calls: bool,
    pub trace_limit: Option<usize>,
    pub trace_filter: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmBranchStats {
    pub jump_if_true_taken: usize,
    pub jump_if_true_not_taken: usize,
    pub jump_if_false_taken: usize,
    pub jump_if_false_not_taken: usize,
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
    pub branch: VmBranchStats,
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
    pub runtime_output_event_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmProcessSpecSnapshot {
    pub spec_id: u32,
    pub type_name: String,
    pub kind: String,
    pub instance: String,
    pub init_fun_idx: u32,
    pub init_policy: String,
    pub state_type: String,
    pub handler_count: usize,
    pub dependency_count: usize,
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
    pub standby_state_pending: bool,
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
    pub worker_sets: Vec<VmWorkerSetSnapshot>,
    pub waiting: BTreeMap<u64, String>,
    pub replies: BTreeMap<u64, u64>,
    pub deadlines: Vec<VmDeadlineSnapshot>,
    pub futures: Vec<VmFutureSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmWorkerSetSnapshot {
    pub id: u64,
    pub worker_process: String,
    pub supervisor: String,
    pub target: i64,
    pub min: i64,
    pub max: i64,
    pub member_pids: Vec<u64>,
    pub live_count: usize,
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

    fn record_branch_outcome(&mut self, kind: &str, pc: usize, target: u32, taken: bool) {
        match (kind, taken) {
            ("JumpIfTrue", true) => self.stats.branch.jump_if_true_taken += 1,
            ("JumpIfTrue", false) => self.stats.branch.jump_if_true_not_taken += 1,
            ("JumpIfFalse", true) => self.stats.branch.jump_if_false_taken += 1,
            ("JumpIfFalse", false) => self.stats.branch.jump_if_false_not_taken += 1,
            _ => {}
        }
        if self.options.trace_opcodes && self.trace_enabled_for(kind) {
            self.push_trace(format!(
                "branch pc={} opcode={} target={} taken={}",
                pc, kind, target, taken
            ));
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
struct ProcessRuntime {
    current_tick_ms: u64,
    next_pid: u64,
    next_workers_id: u64,
    next_future_id: FutureId,
    next_correlation_id: CorrelationId,
    next_detached_task_id: u64,
    specs_by_id: Vec<RuntimeProcessSpec>,
    spec_id_by_name: BTreeMap<String, u32>,
    specs_by_name: BTreeMap<String, RuntimeProcessSpec>,
    handler_contexts: BTreeMap<String, BTreeMap<String, RuntimeHandlerTarget>>,
    singleton_by_name: BTreeMap<String, u64>,
    singleton_inits: BTreeMap<String, SingletonFlight>,
    refilling_worker_sets: BTreeSet<u64>,
    processes: BTreeMap<u64, ProcessInstance>,
    futures: BTreeMap<FutureId, FutureRecord>,
    reply_table: BTreeMap<CorrelationId, FutureId>,
    waiting_table: BTreeMap<u64, ProcessWaitReason>,
    deadline_queue: VecDeque<DeadlineEntry>,
    run_queue: VecDeque<u64>,
    output_events: VecDeque<RuntimeOutputEvent>,
    detached_tasks: BTreeMap<u64, DetachedTask>,
    root_supervisor: RootSupervisorState,
    worker_sets: BTreeMap<u64, WorkerSetState>,
}

#[derive(Debug, Clone)]
struct WorkerSetState {
    supervisor_name: String,
    worker_process: String,
    init_callable: Callable,
    strategy: WorkerStrategyState,
    target: i64,
    members: Vec<u64>,
    next_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerStrategyState {
    init: i64,
    min: i64,
    max: i64,
    scale: WorkerScaleState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkerScaleState {
    Fix(i64),
}

impl WorkerStrategyState {
    fn target(&self) -> i64 {
        match self.scale {
            WorkerScaleState::Fix(size) => size,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ProcessInstance {
    pid: u64,
    spec_id: u32,
    status: ProcessStatus,
    mailbox: VecDeque<ProcessMailboxMessage>,
    execution_context: Option<ExecutionContext>,
    state_value: Option<Value>,
    owner: Option<u64>,
    lifecycle_sink: Option<LifecycleSink>,
    standby_state_pending: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum LifecycleSink {
    Supervisor(String),
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
struct ExecutionContext {
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    tail_call_breadcrumbs: VecDeque<RuntimeStackFrame>,
    continuations: Vec<ContinuationFrame>,
    pending_invocation: Option<Result<Invocation, RuntimeError>>,
    cancellation: Option<RuntimeError>,
    pc: usize,
    target: ExecutionTarget,
}

type ProcessExecutionContext = ExecutionContext;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecutionTarget {
    TopLevel,
    FrameDepth(usize),
}

fn handler_target_identity(target: &RuntimeHandlerTarget) -> String {
    if target.named_args.is_empty() {
        return target.name.clone();
    }
    let args = target
        .named_args
        .iter()
        .map(|arg| format!("{}={}", arg.name, arg.value))
        .collect::<Vec<_>>()
        .join(",");
    format!("{}({})", target.name, args)
}

fn stable_handler_pid(identity: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in identity.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn parse_handler_target_identity(identity: &str) -> (&str, Vec<(String, String)>) {
    let Some(open) = identity.find('(') else {
        return (identity, Vec::new());
    };
    let name = &identity[..open];
    let args = identity
        .strip_suffix(')')
        .map(|s| &s[open + 1..])
        .unwrap_or("")
        .split(',')
        .filter(|item| !item.is_empty())
        .filter_map(|item| {
            let (key, value) = item.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect();
    (name, args)
}

fn runtime_spec_is_standby(spec: &RuntimeProcessSpec) -> bool {
    matches!(spec.init.policy, RuntimeInitPolicy::Standby)
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Budget {
    max_reductions: u64,
    reductions: u64,
}

#[allow(dead_code)]
impl Budget {
    fn new(max_reductions: u64) -> Self {
        Self {
            max_reductions,
            reductions: 0,
        }
    }

    fn consume(&mut self, cost: u64) {
        self.reductions = self.reductions.saturating_add(cost);
    }

    fn expired(&self) -> bool {
        self.reductions >= self.max_reductions
    }

    fn reductions(&self) -> u64 {
        self.reductions
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum ProcessRunOutcome {
    QuantumExpired,
    Halted(Value),
    Pending(FutureId),
    Failed(RuntimeError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpcodeControl {
    Continue,
    Halt,
    Pending {
        future_id: FutureId,
        resume_pc: usize,
    },
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
    effective_supervisors: BTreeMap<String, RuntimeSupervisorPolicy>,
    child_table: BTreeMap<String, Vec<u64>>,
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
            runtime_output_event_count: self.output_events.len(),
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
    /// Logical caller frames overwritten by TCO, newest at the back.
    tail_call_breadcrumbs: VecDeque<RuntimeStackFrame>,
    continuations: Vec<ContinuationFrame>,
    pending_invocation: Option<Result<Invocation, RuntimeError>>,
    cancellation: Option<RuntimeError>,
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
    /// REPL-owned host stdout buffer. This protects the active input line without
    /// changing DSL-visible handler targets or test capture policy.
    repl_host_stdout: Option<Vec<String>>,
    /// REPL-owned host stderr buffer.
    repl_host_stderr: Option<Vec<String>>,
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
    test_runner: VmTestRunner,
    /// Collected test events emitted by the test DSL runtime helpers.
    test_events: Vec<VmTestEvent>,
    /// Cursor tracking for test-event I/O slices.
    test_stdout_cursor: usize,
    test_stderr_cursor: usize,
    open_files: HashMap<u64, VmOpenFile>,
    batch_shutdown_pending: bool,
    next_file_handle_id: u64,
    cwd: PathBuf,
    /// VM-owned process table built from bytecode runtime process specs.
    process_runtime: ProcessRuntime,
    runtime_clock_anchor: Instant,
}

impl VM {
    pub fn new(bytecode: Bytecode) -> Self {
        let num_locals = bytecode.num_locals;
        let process_runtime = ProcessRuntime::from_spec_table(&bytecode.runtime_process_specs);
        let mut vm = Self {
            bytecode,
            stack: Vec::new(),
            frames: vec![CallFrame {
                return_pc: 0,
                stack_base: 0,
                call_site: None,
                trace_frame: None,
                tail_call_breadcrumb_base_len: 0,
                locals: vec![Value::Unit; num_locals],
            }],
            tail_call_breadcrumbs: VecDeque::new(),
            continuations: Vec::new(),
            pending_invocation: None,
            cancellation: None,
            pc: 0,
            source: None,
            source_file: None,
            cli_args: Vec::new(),
            output: None,
            error_output: None,
            io_policy: VmIoPolicy::default(),
            repl_host_stdout: None,
            repl_host_stderr: None,
            stdin_input: None,
            stdin_input_cursor: 0,
            exit_code: 0,
            last_result: None,
            observer: None,
            test_runner: VmTestRunner::default(),
            test_events: Vec::new(),
            test_stdout_cursor: 0,
            test_stderr_cursor: 0,
            open_files: HashMap::new(),
            batch_shutdown_pending: false,
            next_file_handle_id: 1,
            cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            process_runtime,
            runtime_clock_anchor: Instant::now(),
        };
        vm.apply_runtime_supervisor_overrides();
        vm
    }

    /// Create an empty VM intended for REPL/incremental execution.
    pub(crate) fn new_interactive(type_registry: TypeRegistry) -> Self {
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

    /// Retain an immutable source for encoded compiler spans and saved images.
    pub fn register_source(
        &mut self,
        entry: sindr::ir::SourceFileEntry,
    ) -> Result<(), RuntimeError> {
        if let Some(existing) = self
            .bytecode
            .sources
            .iter()
            .find(|source| source.source_id == entry.source_id)
        {
            if existing
                .normalized_path
                .as_deref()
                .unwrap_or(&existing.path)
                != entry.normalized_path.as_deref().unwrap_or(&entry.path)
                || existing.text != entry.text
            {
                return Err(RuntimeError::new(format!(
                    "Source ID {} already has different source metadata",
                    entry.source_id
                )));
            }
        } else {
            self.bytecode.sources.push(entry);
        }
        Ok(())
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
            origin_source: None,
            delegate_function: None,
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
                    origin_source: None,
                    delegate_function: None,
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
            origin_source: None,
            delegate_function: None,
        }
    }

    fn callable_for_function(&self, fun_idx: u32) -> Callable {
        Callable {
            target: CallableTarget::Function(fun_idx),
            lexical_captures: Vec::new(),
            metadata: self.callable_metadata_for_function(fun_idx),
        }
    }

    fn callable_metadata_for_template(&self, template_id: u32) -> CallableMetadata {
        self.callable_template(template_id)
            .map(|template| CallableMetadata {
                origin: template.metadata.origin,
                module: template.metadata.module.clone(),
                name: template.metadata.name.clone(),
                full_signature: template.metadata.full_signature.clone(),
                origin_source: None,
                delegate_function: None,
            })
            .unwrap_or_default()
    }

    fn callable_template(&self, template_id: u32) -> Result<&CallableTemplate, RuntimeError> {
        self.bytecode
            .callable_templates
            .iter()
            .find(|template| template.template_id == template_id)
            .ok_or_else(|| RuntimeError::new(format!("Unknown callable template: {}", template_id)))
    }

    pub fn runtime_error_location(&self) -> Option<Location> {
        let (span_start, span_end) = self.current_frame().ok()?.call_site?;
        self.location_for_span(span_start, span_end, "<runtime>")
    }

    fn location_for_span(
        &self,
        span_start: u32,
        span_end: u32,
        func: impl Into<String>,
    ) -> Option<Location> {
        let (file, source, span_start, span_end) =
            match sindr::ir::decode_module_source_span(span_start as usize, span_end as usize) {
                Some((source_id, start, end)) => {
                    let Some(entry) = self
                        .bytecode
                        .sources
                        .iter()
                        .find(|entry| entry.source_id == source_id)
                    else {
                        return Some(Location {
                            file: format!("<source:{source_id}>"),
                            func: func.into(),
                            line: 0,
                            column: 0,
                            span_start,
                            span_end,
                        });
                    };
                    (
                        entry
                            .normalized_path
                            .as_deref()
                            .unwrap_or(&entry.path)
                            .to_string(),
                        entry.text.as_deref(),
                        u32::try_from(start).ok()?,
                        u32::try_from(end).ok()?,
                    )
                }
                None => (
                    self.source_file().unwrap_or("<runtime>").to_string(),
                    self.source(),
                    span_start,
                    span_end,
                ),
            };
        let (line, column) = source
            .map(|source| line_column_for_offset(source, span_start as usize))
            .unwrap_or((0, 0));
        Some(Location {
            file,
            func: func.into(),
            line,
            column,
            span_start,
            span_end,
        })
    }

    pub(crate) fn current_stack_trace_snapshot(&self) -> Vec<RuntimeStackFrame> {
        let mut stack_trace = Vec::new();
        let mut breadcrumb_upper = self.tail_call_breadcrumbs.len();
        for frame in self.frames.iter().rev() {
            if let Some(trace_frame) = frame.trace_frame.clone() {
                stack_trace.push(trace_frame);
            }
            let breadcrumb_start = frame.tail_call_breadcrumb_base_len.min(breadcrumb_upper);
            stack_trace.extend(
                self.tail_call_breadcrumbs
                    .range(breadcrumb_start..breadcrumb_upper)
                    .rev()
                    .cloned(),
            );
            breadcrumb_upper = breadcrumb_start;
        }
        stack_trace
    }

    fn stack_trace_snapshot_without_head_function(&self, function: &str) -> Vec<RuntimeStackFrame> {
        let mut stack_trace = self.current_stack_trace_snapshot();
        let function_tail = function.rsplit("::").next();
        if stack_trace.first().is_some_and(|frame| {
            frame
                .function
                .as_deref()
                .and_then(|name| name.rsplit("::").next())
                == function_tail
        }) {
            stack_trace.remove(0);
        }
        stack_trace
    }

    fn function_trace_name(&self, entry: &FunctionEntry) -> String {
        entry
            .qualified_name
            .as_deref()
            .and_then(|name| name.rsplit("::").next())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("fun#{}", entry.fun_idx))
    }

    fn trace_frame_for_call(
        &self,
        entry: &FunctionEntry,
        call_kind: RuntimeCallKind,
        span_start: u32,
        span_end: u32,
        tco: bool,
    ) -> RuntimeStackFrame {
        let function = self.function_trace_name(entry);
        RuntimeStackFrame {
            phase: RuntimeExecutionPhase::Runtime,
            function: Some(function.clone()),
            fun_idx: Some(entry.fun_idx),
            call_kind,
            location: self.location_for_span(span_start, span_end, function),
            process: None,
            tco,
        }
    }

    fn trace_frame_for_builtin(
        &self,
        builtin_name: &str,
        span_start: u32,
        span_end: u32,
    ) -> RuntimeStackFrame {
        RuntimeStackFrame {
            phase: RuntimeExecutionPhase::Runtime,
            function: Some(builtin_name.to_string()),
            fun_idx: None,
            call_kind: RuntimeCallKind::Builtin,
            location: self.location_for_span(span_start, span_end, builtin_name),
            process: None,
            tco: false,
        }
    }

    fn trace_frame_for_callable_template(
        &self,
        template_id: u32,
        span_start: u32,
        span_end: u32,
    ) -> RuntimeStackFrame {
        let function = self
            .callable_template(template_id)
            .ok()
            .and_then(|template| template.metadata.name.clone())
            .unwrap_or_else(|| format!("template#{}", template_id));
        RuntimeStackFrame {
            phase: RuntimeExecutionPhase::Runtime,
            function: Some(function.clone()),
            fun_idx: None,
            call_kind: RuntimeCallKind::CallableTemplate,
            location: self.location_for_span(span_start, span_end, function),
            process: None,
            tco: false,
        }
    }

    fn trace_frame_for_task(
        &self,
        function: impl Into<String>,
        span_start: u32,
        span_end: u32,
    ) -> RuntimeStackFrame {
        let function = function.into();
        RuntimeStackFrame {
            phase: RuntimeExecutionPhase::Runtime,
            function: Some(function.clone()),
            fun_idx: None,
            call_kind: RuntimeCallKind::Task,
            location: self.location_for_span(span_start, span_end, function),
            process: None,
            tco: false,
        }
    }

    fn task_mode_trace_name(mode: TaskMode) -> &'static str {
        match mode {
            TaskMode::Call => "Task::call",
            TaskMode::Async => "Task::async",
            TaskMode::Launch => "Task::launch",
            TaskMode::Cast => "Task::cast",
        }
    }

    fn record_tail_call_breadcrumb(&mut self) {
        let Some(mut frame) = self
            .current_frame()
            .ok()
            .and_then(|frame| frame.trace_frame.clone())
        else {
            return;
        };
        frame.tco = true;
        if self.tail_call_breadcrumbs.len() >= MAX_TAIL_CALL_TRACE_BREADCRUMBS {
            self.tail_call_breadcrumbs.pop_front();
        }
        self.tail_call_breadcrumbs.push_back(frame);
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
            stack_trace: self.current_stack_trace_snapshot(),
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
        if !err_context.stack_trace.is_empty() {
            context.stack_trace = err_context.stack_trace;
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

    /// Route host terminal stdout/stderr through REPL-owned buffers.
    ///
    /// This is intentionally separate from `VmIoPolicy`: REPL terminal
    /// protection must not override DSL-visible standard I/O handler targets or
    /// test capture behavior.
    pub fn enable_repl_host_io_buffering(&mut self) {
        if self.repl_host_stdout.is_none() {
            self.repl_host_stdout = Some(Vec::new());
        }
        if self.repl_host_stderr.is_none() {
            self.repl_host_stderr = Some(Vec::new());
        }
    }

    pub fn take_repl_host_stdout(&mut self) -> Vec<String> {
        match self.repl_host_stdout.as_mut() {
            Some(buffer) => std::mem::take(buffer),
            None => Vec::new(),
        }
    }

    pub fn take_repl_host_stderr(&mut self) -> Vec<String> {
        match self.repl_host_stderr.as_mut() {
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

    fn emit_host_stdout_line(&mut self, line: String) {
        if let Some(buffer) = self.repl_host_stdout.as_mut() {
            buffer.push(line);
        } else {
            println!("{}", line);
        }
    }

    fn emit_host_stdout_text(&mut self, text: String) -> io::Result<()> {
        if let Some(buffer) = self.repl_host_stdout.as_mut() {
            if !text.is_empty() {
                buffer.push(text);
            }
            Ok(())
        } else {
            print!("{}", text);
            io::stdout().flush()
        }
    }

    fn emit_host_stderr_line(&mut self, line: String) {
        if let Some(buffer) = self.repl_host_stderr.as_mut() {
            buffer.push(line);
        } else {
            eprintln!("{}", line);
        }
    }

    fn emit_host_stderr_text(&mut self, text: String) -> io::Result<()> {
        if let Some(buffer) = self.repl_host_stderr.as_mut() {
            if !text.is_empty() {
                buffer.push(text);
            }
            Ok(())
        } else {
            eprint!("{}", text);
            io::stderr().flush()
        }
    }

    pub(crate) fn emit_stdout_line(&mut self, line: String) {
        self.process_runtime
            .output_events
            .push_back(RuntimeOutputEvent::StdOut(format!("{line}\n")));
        match self.io_policy.stdout {
            IoMode::Passthrough => self.emit_host_stdout_line(line),
            IoMode::Capture => {
                if let Some(buffer) = self.output.as_mut() {
                    buffer.push(line);
                } else {
                    self.emit_host_stdout_line(line);
                }
            }
            IoMode::Tee => {
                self.emit_host_stdout_line(line.clone());
                if let Some(buffer) = self.output.as_mut() {
                    buffer.push(line);
                }
            }
        }
    }

    pub(crate) fn emit_stdout_text(&mut self, text: String) -> io::Result<()> {
        if !text.is_empty() {
            self.process_runtime
                .output_events
                .push_back(RuntimeOutputEvent::StdOut(text.clone()));
        }
        match self.io_policy.stdout {
            IoMode::Passthrough => self.emit_host_stdout_text(text),
            IoMode::Capture => {
                if let Some(buffer) = self.output.as_mut() {
                    if !text.is_empty() {
                        buffer.push(text);
                    }
                    Ok(())
                } else {
                    self.emit_host_stdout_text(text)
                }
            }
            IoMode::Tee => {
                self.emit_host_stdout_text(text.clone())?;
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
        self.process_runtime
            .output_events
            .push_back(RuntimeOutputEvent::StdErr(format!("{line}\n")));
        match self.io_policy.stderr {
            IoMode::Passthrough => self.emit_host_stderr_line(line),
            IoMode::Capture => {
                if let Some(buffer) = self.error_output.as_mut() {
                    buffer.push(line);
                } else {
                    self.emit_host_stderr_line(line);
                }
            }
            IoMode::Tee => {
                self.emit_host_stderr_line(line.clone());
                if let Some(buffer) = self.error_output.as_mut() {
                    buffer.push(line);
                }
            }
        }
    }

    pub(crate) fn emit_stderr_text(&mut self, text: String) -> io::Result<()> {
        if !text.is_empty() {
            self.process_runtime
                .output_events
                .push_back(RuntimeOutputEvent::StdErr(text.clone()));
        }
        match self.io_policy.stderr {
            IoMode::Passthrough => self.emit_host_stderr_text(text),
            IoMode::Capture => {
                if let Some(buffer) = self.error_output.as_mut() {
                    if !text.is_empty() {
                        buffer.push(text);
                    }
                    Ok(())
                } else {
                    self.emit_host_stderr_text(text)
                }
            }
            IoMode::Tee => {
                self.emit_host_stderr_text(text.clone())?;
                if let Some(buffer) = self.error_output.as_mut() {
                    if !text.is_empty() {
                        buffer.push(text);
                    }
                }
                Ok(())
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

    pub(crate) fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub(crate) fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    pub(crate) fn resolve_host_path(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.cwd.join(path)
        }
    }

    fn boot_failure_error(&self, process_name: &str, detail: &str) -> RuntimeError {
        RuntimeError::new(format!("process `{process_name}` failed to boot: {detail}"))
    }

    fn with_vm_init_process_context(
        mut err: RuntimeError,
        process_name: &str,
        init_policy: &str,
        trigger: &str,
    ) -> RuntimeError {
        err.context.details.push("phase=VM::Init".into());
        err.context
            .details
            .push(format!("process_name={process_name}"));
        err.context
            .details
            .push(format!("init_policy={init_policy}"));
        err.context.details.push(format!("trigger={trigger}"));
        err
    }

    fn duration_millis(&self, value: &Value, context: &str) -> Result<u64, RuntimeError> {
        let Value::Tagged { tag, fields } = value else {
            return Err(RuntimeError::process_init_failed(format!(
                "{context} expects Duration, got {value:?}"
            )));
        };
        let Some(entry) = self.type_registry().lookup(*tag) else {
            return Err(RuntimeError::process_init_failed(format!(
                "{context} references unknown Duration tag {tag}"
            )));
        };
        if !entry.name.ends_with("Duration") {
            return Err(RuntimeError::process_init_failed(format!(
                "{context} expects Duration, got {}",
                entry.name
            )));
        }
        let Some(Value::Int(ms)) = fields.first() else {
            return Err(RuntimeError::process_init_failed(format!(
                "{context} expects Duration milliseconds payload, got {fields:?}"
            )));
        };
        ms.to_u64().ok_or_else(|| {
            RuntimeError::process_init_failed(format!(
                "{context} duration is out of range for u64 milliseconds: {ms}"
            ))
        })
    }

    fn ensure_root_supervisor_booted(&mut self) -> Result<(), RuntimeError> {
        if self.process_runtime.root_supervisor.boot_completed {
            return Ok(());
        }

        self.apply_runtime_handler_overrides()?;
        self.apply_runtime_supervisor_overrides();
        let limits = self.bytecode.runtime_boot_plan.runtime_limits.clone();
        let boot_specs = if self.bytecode.runtime_boot_plan.has_explicit_entries() {
            for entry in &self.bytecode.runtime_boot_plan.singletons {
                let Some(spec) = self.process_runtime.specs_by_name.get(&entry.process_name) else {
                    return Err(self.boot_failure_error(
                        &entry.process_name,
                        "singleton process is not defined or not visible",
                    ));
                };
                if spec.instance != RuntimeProcessInstance::Singleton {
                    return Err(self.boot_failure_error(
                        &entry.process_name,
                        "only Singleton process can appear in singleton boot entry",
                    ));
                }
                if entry.init_timeout_ms < limits.min_init_timeout_ms {
                    return Err(self.boot_failure_error(
                        &entry.process_name,
                        "init timeout must be at least `1ms`",
                    ));
                }
                if entry.init_timeout_ms > limits.max_init_timeout_ms {
                    return Err(self.boot_failure_error(
                        &entry.process_name,
                        "init timeout must not exceed `60s`",
                    ));
                }
            }
            self.bytecode
                .runtime_boot_plan
                .singletons
                .iter()
                .filter_map(|entry| {
                    self.process_runtime
                        .specs_by_name
                        .get(&entry.process_name)
                        .cloned()
                })
                .filter(|spec| {
                    spec.instance == RuntimeProcessInstance::Singleton
                        && !self
                            .process_runtime
                            .singleton_by_name
                            .contains_key(&spec.type_name)
                })
                .map(|spec| {
                    let timeout_ms = self
                        .bytecode
                        .runtime_boot_plan
                        .singletons
                        .iter()
                        .find(|entry| entry.process_name == spec.type_name)
                        .map(|entry| entry.init_timeout_ms)
                        .unwrap_or(limits.default_init_timeout_ms);
                    (spec, timeout_ms)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let saved_runtime = self.process_runtime.clone();
        for (spec, timeout_ms) in boot_specs {
            if let Err(err) =
                self.ensure_singleton_available_with_timeout(&spec.type_name, Some(timeout_ms))
            {
                let detail = self
                    .process_runtime
                    .root_supervisor
                    .boot_failures
                    .get(&spec.type_name)
                    .cloned()
                    .unwrap_or_else(|| err.message.clone());
                self.process_runtime = saved_runtime;
                self.process_runtime
                    .root_supervisor
                    .boot_failures
                    .insert(spec.type_name.clone(), detail.clone());
                return Err(err);
            }
        }

        self.process_runtime.root_supervisor.boot_completed = true;
        Ok(())
    }

    fn apply_runtime_handler_overrides(&mut self) -> Result<(), RuntimeError> {
        for override_entry in &self.bytecode.runtime_boot_plan.handler_overrides {
            let Some(slots) = self
                .process_runtime
                .handler_contexts
                .get_mut(&override_entry.target_process)
            else {
                return Err(self.boot_failure_error(
                    &override_entry.target_process,
                    "handler override target process is not defined or not visible",
                ));
            };
            if !slots.contains_key(&override_entry.slot) {
                return Err(self.boot_failure_error(
                    &override_entry.target_process,
                    "handler slot is not declared by the target process",
                ));
            }
            slots.insert(
                override_entry.slot.clone(),
                override_entry.handler_target.clone(),
            );
        }
        Ok(())
    }

    fn apply_runtime_supervisor_overrides(&mut self) {
        for override_entry in &self.bytecode.runtime_boot_plan.supervisor_overrides {
            self.process_runtime
                .root_supervisor
                .effective_supervisors
                .insert(
                    override_entry.process_name.clone(),
                    override_entry.policy.clone(),
                );
            if override_entry
                .process_name
                .rsplit("::")
                .next()
                .is_some_and(|name| name == "DynamicSupervisor")
            {
                self.process_runtime
                    .root_supervisor
                    .effective_supervisors
                    .insert("DynamicSupervisor".into(), override_entry.policy.clone());
            }
        }
    }

    fn ensure_singleton_available_with_timeout(
        &mut self,
        process_name: &str,
        timeout_ms: Option<u64>,
    ) -> Result<u64, RuntimeError> {
        let outcome = self.start_singleton(process_name.to_string(), timeout_ms)?;
        match self.drive_builtin_outcome(outcome)? {
            Value::Pid(pid) => Ok(pid.id),
            _ => Err(RuntimeError::new("singleton init did not return PID")),
        }
    }

    pub(crate) fn process_context_handler(
        &mut self,
        process_name: String,
        slot: String,
    ) -> Result<Value, RuntimeError> {
        let Some(target) = self
            .process_runtime
            .handler_targets_for_process(&process_name)
            .and_then(|slots| slots.get(&slot))
        else {
            return Err(RuntimeError::new(format!(
                "handler slot `{slot}` is not declared for process `{process_name}`"
            )));
        };
        let identity = handler_target_identity(target);
        Ok(Value::Pid(PidHandle {
            id: stable_handler_pid(&identity),
            process_name: identity,
        }))
    }

    pub(crate) fn out_handler_write(
        &mut self,
        pid: &PidHandle,
        text: String,
    ) -> Result<Value, RuntimeError> {
        match parse_handler_target_identity(&pid.process_name) {
            ("StdOut", _) => {
                self.emit_stdout_text(text).map_err(|err| {
                    RuntimeError::new(format!("StdOut handler write failed: {err}"))
                })?;
                Ok(ok_vm_result(Value::Unit))
            }
            ("StdErr", _) => {
                self.emit_stderr_text(text).map_err(|err| {
                    RuntimeError::new(format!("StdErr handler write failed: {err}"))
                })?;
                Ok(ok_vm_result(Value::Unit))
            }
            ("NullOutHandler", _) => Ok(ok_vm_result(Value::Unit)),
            ("FileOutHandler", args) => {
                let Some(path) = args
                    .iter()
                    .find_map(|(name, value)| (name == "path").then_some(value))
                else {
                    return Ok(err_vm_result(self.process_error(
                        "HandlerInitFailed",
                        "FileOutHandler requires named argument `path`",
                    )));
                };
                use std::fs::OpenOptions;
                let mut file = match OpenOptions::new().create(true).append(true).open(path) {
                    Ok(file) => file,
                    Err(err) => {
                        return Ok(err_vm_result(self.process_error(
                            "HandlerInitFailed",
                            &format!("FileOutHandler open failed: {err}"),
                        )));
                    }
                };
                if let Err(err) = file.write_all(text.as_bytes()) {
                    return Ok(err_vm_result(self.process_error(
                        "HandlerWriteFailed",
                        &format!("FileOutHandler write failed: {err}"),
                    )));
                }
                Ok(ok_vm_result(Value::Unit))
            }
            (other, _) => Ok(err_vm_result(self.process_error(
                "UnknownHandlerTarget",
                &format!("unknown OutHandler target `{other}`"),
            ))),
        }
    }

    pub(crate) fn supervisor_adopt(
        &mut self,
        supervisor_name: String,
        pid: PidHandle,
    ) -> Result<Value, RuntimeError> {
        let policy = self.effective_supervisor_policy(&supervisor_name)?;
        if !policy.allow_adopt {
            return Ok(err_vm_result(self.process_error(
                "SupervisorAdoptForbidden",
                &format!("{supervisor_name} does not allow adopt"),
            )));
        }
        let Some(entry) = self.process_runtime.processes.get(&pid.id) else {
            return Ok(err_vm_result(self.process_error(
                "InvalidPid",
                &format!("unknown pid {} for {}", pid.id, pid.process_name),
            )));
        };
        if !self.is_adoptable_worker(entry) {
            return Ok(err_vm_result(self.process_error(
                "SupervisorAdoptInvalidPid",
                &format!(
                    "supervisor adopt accepts only Worker PID with live state, got {}",
                    pid.process_name
                ),
            )));
        }

        let old_supervisor = match &entry.lifecycle_sink {
            Some(LifecycleSink::Supervisor(old_supervisor)) => Some(old_supervisor.clone()),
            None => None,
        };
        if old_supervisor.as_deref() != Some(supervisor_name.as_str()) {
            if let Some(old_supervisor) = old_supervisor {
                self.remove_supervisor_child(&old_supervisor, pid.id);
            }
        }
        let Some(entry) = self.process_runtime.processes.get_mut(&pid.id) else {
            return Err(RuntimeError::new(format!(
                "process {} disappeared during supervisor adopt",
                pid.id
            )));
        };
        entry.lifecycle_sink = Some(LifecycleSink::Supervisor(supervisor_name.clone()));
        self.add_supervisor_child(&supervisor_name, pid.id);
        Ok(ok_vm_result(Value::Unit))
    }

    pub(crate) fn supervisor_status(
        &mut self,
        supervisor_name: String,
    ) -> Result<Value, RuntimeError> {
        let policy = self.effective_supervisor_policy(&supervisor_name)?;
        let child_count = self.unique_live_supervisor_child_count(&supervisor_name);
        let shutdown_timeout =
            self.supervisor_shutdown_timeout_value(policy.shutdown_timeout_ms)?;
        let Some(tag) = self.type_registry().tag_by_name("SupervisorStatus") else {
            return Err(RuntimeError::new("SupervisorStatus type is not registered"));
        };
        Ok(ok_vm_result(Value::Tagged {
            tag,
            fields: vec![
                Value::Str(
                    supervisor_name
                        .strip_prefix("Global::")
                        .unwrap_or(&supervisor_name)
                        .to_string(),
                ),
                Value::Int(int(child_count)),
                Value::Str(policy.strategy),
                Value::Int(int(policy.max_restarts as i64)),
                Value::Int(int(policy.max_seconds as i64)),
                Value::Bool(policy.allow_adopt),
                shutdown_timeout,
            ],
        }))
    }

    fn effective_supervisor_policy(
        &self,
        supervisor_name: &str,
    ) -> Result<RuntimeSupervisorPolicy, RuntimeError> {
        let supervisor_short_name = supervisor_name
            .rsplit("::")
            .next()
            .unwrap_or(supervisor_name);
        if let Some(override_entry) = self
            .bytecode
            .runtime_boot_plan
            .supervisor_overrides
            .iter()
            .rev()
            .find(|entry| {
                entry.process_name == supervisor_name
                    || entry
                        .process_name
                        .rsplit("::")
                        .next()
                        .is_some_and(|name| name == supervisor_short_name)
            })
        {
            return Ok(override_entry.policy.clone());
        }
        self.process_runtime
            .root_supervisor
            .effective_supervisors
            .get(supervisor_name)
            .cloned()
            .ok_or_else(|| RuntimeError::new(format!("unknown supervisor `{supervisor_name}`")))
    }

    fn supervisor_shutdown_timeout_value(
        &self,
        timeout_ms: Option<u64>,
    ) -> Result<Value, RuntimeError> {
        let Some(none_tag) = self
            .type_registry()
            .tag_by_name(sindr::builtin::OPTION_NONE_VARIANT.qualified_name)
        else {
            return Err(RuntimeError::new(
                "Option::None type is not registered for SupervisorStatus",
            ));
        };
        let Some(some_tag) = self
            .type_registry()
            .tag_by_name(sindr::builtin::OPTION_SOME_VARIANT.qualified_name)
        else {
            return Err(RuntimeError::new(
                "Option::Some type is not registered for SupervisorStatus",
            ));
        };
        match timeout_ms {
            Some(ms) => {
                let Some(duration_tag) = self.type_registry().tag_by_name("Duration") else {
                    return Err(RuntimeError::new(
                        "Duration type is not registered for SupervisorStatus",
                    ));
                };
                Ok(Value::Tagged {
                    tag: some_tag,
                    fields: vec![
                        Value::Int(int(sindr::builtin::OPTION_SOME_VARIANT.discriminant)),
                        Value::Tagged {
                            tag: duration_tag,
                            fields: vec![Value::Int(int(ms))],
                        },
                    ],
                })
            }
            None => Ok(Value::Tagged {
                tag: none_tag,
                fields: vec![Value::Int(int(
                    sindr::builtin::OPTION_NONE_VARIANT.discriminant
                ))],
            }),
        }
    }

    fn decode_worker_strategy(&self, value: &Value) -> Result<WorkerStrategyState, String> {
        let Value::Tagged { tag, fields } = value else {
            return Err("worker strategy must be a WorkerStrategy value".into());
        };
        let Some(entry) = self.type_registry().lookup(*tag) else {
            return Err("worker strategy has unknown runtime tag".into());
        };
        if !Self::runtime_type_named(&entry.name, "WorkerStrategy") {
            return Err("worker strategy must be a WorkerStrategy value".into());
        }
        let init = Self::worker_strategy_int_field(entry, fields, "init")?;
        let min = Self::worker_strategy_int_field(entry, fields, "min")?;
        let max = Self::worker_strategy_int_field(entry, fields, "max")?;
        let scale_value = Self::worker_strategy_field(entry, fields, "scale")
            .ok_or_else(|| "worker strategy missing scale".to_string())?;
        let scale = self.decode_worker_scale(scale_value)?;
        Ok(WorkerStrategyState {
            init,
            min,
            max,
            scale,
        })
    }

    fn decode_worker_scale(&self, value: &Value) -> Result<WorkerScaleState, String> {
        let Value::Tagged { tag, fields } = value else {
            return Err("worker scale must be WorkerScale::Fix".into());
        };
        let Some(entry) = self.type_registry().lookup(*tag) else {
            return Err("worker scale has unknown runtime tag".into());
        };
        if !Self::runtime_type_named(&entry.name, "WorkerScale::Fix") {
            return Err("worker scale must be WorkerScale::Fix".into());
        }
        let Some(Value::Int(size)) = fields.get(1) else {
            return Err("WorkerScale::Fix must contain an Int size".into());
        };
        let Some(size) = size.to_i64() else {
            return Err("WorkerScale::Fix size must be representable as i64".into());
        };
        Ok(WorkerScaleState::Fix(size))
    }

    fn worker_strategy_int_field(
        entry: &sindr::runtime::TypeEntry,
        fields: &[Value],
        name: &str,
    ) -> Result<i64, String> {
        let Some(Value::Int(value)) = Self::worker_strategy_field(entry, fields, name) else {
            return Err(format!("worker strategy missing Int field `{name}`"));
        };
        value
            .to_i64()
            .ok_or_else(|| format!("worker strategy field `{name}` must be representable as i64"))
    }

    fn worker_strategy_field<'a>(
        entry: &sindr::runtime::TypeEntry,
        fields: &'a [Value],
        name: &str,
    ) -> Option<&'a Value> {
        let index = entry.field_names.iter().position(|field| field == name)?;
        fields.get(index)
    }

    fn runtime_type_named(actual: &str, expected: &str) -> bool {
        let actual = actual.strip_prefix("Global::").unwrap_or(actual);
        actual == expected
            || actual
                .strip_suffix(expected)
                .map(|prefix| prefix.ends_with("::"))
                .unwrap_or(false)
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
        if spec.type_name != pid.process_name {
            let actual_name = spec.type_name.clone();
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
        if entry.standby_state_pending {
            return Err(RuntimeError::process_init_failed(format!(
                "standby process {} reached runtime before init completed",
                pid.id
            )));
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
        if spec.type_name != pid.process_name {
            let actual_name = spec.type_name.clone();
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
        entry.standby_state_pending = false;
        Ok(ok_vm_result(Value::Unit))
    }

    pub(crate) fn genserver_call_reply(
        &mut self,
        pid: &PidHandle,
        next_state: Value,
        reply: Value,
    ) -> Result<Value, RuntimeError> {
        let _ = self.process_store(pid, next_state)?;
        Ok(ok_vm_result(reply))
    }

    pub(crate) fn genserver_call_stop_normal(
        &mut self,
        pid: &PidHandle,
        reply: Value,
    ) -> Result<Value, RuntimeError> {
        let _ = self.finalize_process_stop(pid.id, Some(ok_vm_result(reply.clone())), false);
        Ok(ok_vm_result(reply))
    }

    pub(crate) fn genserver_call_stop_error(
        &mut self,
        pid: &PidHandle,
        err: RichError,
    ) -> Result<Value, RuntimeError> {
        let err_value = err_vm_result(err.clone());
        let _ = self.finalize_process_stop(pid.id, Some(err_value.clone()), false);
        Ok(err_value)
    }

    pub(crate) fn genserver_cast_next(
        &mut self,
        pid: &PidHandle,
        next_state: Value,
    ) -> Result<Value, RuntimeError> {
        let _ = self.process_store(pid, next_state)?;
        Ok(ok_vm_result(Value::Unit))
    }

    pub(crate) fn genserver_cast_stop_normal(
        &mut self,
        pid: &PidHandle,
    ) -> Result<Value, RuntimeError> {
        let _ = self.finalize_process_stop(pid.id, None, false);
        Ok(ok_vm_result(Value::Unit))
    }

    pub(crate) fn genserver_cast_stop_error(
        &mut self,
        pid: &PidHandle,
        err: RichError,
    ) -> Result<Value, RuntimeError> {
        let _ = self.finalize_process_stop(pid.id, Some(err_vm_result(err)), false);
        Ok(ok_vm_result(Value::Unit))
    }

    fn remove_worker_from_sets(&mut self, pid: u64) {
        let mut refill_ids = Vec::new();
        for (workers_id, state) in &mut self.process_runtime.worker_sets {
            let before = state.members.len();
            state.members.retain(|member| *member != pid);
            if state.next_index >= state.members.len() {
                state.next_index = 0;
            }
            if state.members.len() != before && state.members.len() < state.target as usize {
                refill_ids.push(*workers_id);
            }
        }
        for workers_id in refill_ids {
            self.refill_worker_set(workers_id);
        }
    }

    fn remove_process_deadlines(&mut self, pid: u64) {
        self.process_runtime.deadline_queue.retain(|entry| {
            self.process_runtime
                .futures
                .get(&entry.future_id)
                .is_some_and(|future| future.owner != Some(pid))
        });
    }

    fn resolve_owned_process_futures(&mut self, pid: u64, skip_future_id: Option<FutureId>) {
        let owned_futures = self
            .process_runtime
            .futures
            .iter()
            .filter_map(|(future_id, future)| {
                (future.owner == Some(pid)
                    && matches!(future.state, FutureState::Running)
                    && Some(*future_id) != skip_future_id)
                    .then_some(*future_id)
            })
            .collect::<Vec<_>>();
        for future_id in owned_futures {
            self.resolve_future_process_down(future_id, pid);
        }
    }

    fn process_reply_future_for_pid(&self, pid: u64) -> Option<(CorrelationId, FutureId)> {
        let ProcessStatus::Waiting(ProcessWaitReason::Reply(correlation_id)) = self
            .process_runtime
            .processes
            .get(&pid)
            .map(|entry| entry.status.clone())?
        else {
            return None;
        };
        self.process_runtime
            .reply_table
            .get(&correlation_id)
            .copied()
            .map(|future_id| (correlation_id, future_id))
    }

    fn finalize_process_stop(
        &mut self,
        pid: u64,
        reply_value: Option<Value>,
        _from_callback_timeout: bool,
    ) -> Vec<u64> {
        let primary_reply = self.process_reply_future_for_pid(pid);
        let resumed = if let (Some(value), Some((correlation_id, _))) = (reply_value, primary_reply)
        {
            self.process_runtime.resolve_reply(correlation_id, value)
        } else {
            Vec::new()
        };
        let skip_future_id = primary_reply.map(|(_, future_id)| future_id);
        self.resolve_owned_process_futures(pid, skip_future_id);
        self.remove_process_deadlines(pid);
        self.remove_process_detached_tasks(pid);
        self.remove_worker_from_sets(pid);
        let supervisor_name = self
            .process_runtime
            .processes
            .get(&pid)
            .and_then(|entry| entry.lifecycle_sink.clone())
            .and_then(|sink| match sink {
                LifecycleSink::Supervisor(name) => Some(name),
            });
        if let Some(supervisor_name) = supervisor_name {
            self.remove_supervisor_child(&supervisor_name, pid);
        }
        self.process_runtime.waiting_table.remove(&pid);
        if let Some(entry) = self.process_runtime.processes.get_mut(&pid) {
            entry.status = ProcessStatus::Stopped;
            entry.mailbox.clear();
            entry.execution_context = None;
            entry.state_value = None;
            entry.standby_state_pending = false;
        }
        resumed
    }

    pub(crate) fn workers_size(&self, handle: &WorkersHandle) -> Result<Value, RuntimeError> {
        let Some(state) = self.process_runtime.worker_sets.get(&handle.id) else {
            return Err(RuntimeError::new(format!(
                "unknown workers handle {} for {}",
                handle.id, handle.process_name
            )));
        };
        Ok(Value::Int(int(state.members.len() as i64)))
    }

    pub(crate) fn workers_reserve(
        &mut self,
        handle: &WorkersHandle,
    ) -> Result<Value, RuntimeError> {
        let pid = self.next_workers_pid(handle)?;
        Ok(ok_vm_result(Value::WorkerLease(WorkerLeaseHandle {
            workers_id: handle.id,
            pid,
        })))
    }

    fn next_workers_pid(&mut self, handle: &WorkersHandle) -> Result<PidHandle, RuntimeError> {
        let Some(state) = self.process_runtime.worker_sets.get_mut(&handle.id) else {
            return Err(RuntimeError::new(format!(
                "unknown workers handle {} for {}",
                handle.id, handle.process_name
            )));
        };
        if state.members.is_empty() {
            return Err(RuntimeError::new(format!(
                "workers handle {} for {} has no members",
                handle.id, handle.process_name
            )));
        }
        let member_id = state.members[state.next_index % state.members.len()];
        state.next_index = (state.next_index + 1) % state.members.len();
        let Some(process) = self.process_runtime.processes.get(&member_id) else {
            return Err(RuntimeError::new(format!(
                "worker pid {} is not registered",
                member_id
            )));
        };
        let Some(spec) = self.process_runtime.spec_for_id(process.spec_id) else {
            return Err(RuntimeError::new(format!(
                "worker pid {} references unknown spec {}",
                member_id, process.spec_id
            )));
        };
        Ok(PidHandle {
            id: member_id,
            process_name: spec.type_name.clone(),
        })
    }

    pub(crate) fn pid_handle_like(&self, value: &Value) -> Option<PidHandle> {
        match value {
            Value::Pid(pid) => Some(pid.clone()),
            Value::WorkerLease(lease) => Some(lease.pid.clone()),
            _ => None,
        }
    }

    pub(crate) fn pid_handle_like_from_result(&self, value: Value) -> Result<PidHandle, Value> {
        match decode_ok_pid_result(value) {
            Some(pid) => Ok(pid),
            None => Err(err_vm_result(
                self.process_error("InvalidPid", "expected Ok(PID(...)) result"),
            )),
        }
    }

    fn allocate_process_state(
        &mut self,
        name: String,
        state: Option<Value>,
    ) -> Result<u64, RuntimeError> {
        self.allocate_process_instance(name, state, None, None)
    }

    fn allocate_supervised_worker(
        &mut self,
        name: String,
        state: Option<Value>,
        supervisor_name: String,
    ) -> Result<u64, RuntimeError> {
        let pid = self.allocate_process_instance(
            name,
            state,
            None,
            Some(LifecycleSink::Supervisor(supervisor_name)),
        )?;
        if let Some(LifecycleSink::Supervisor(supervisor_name)) = self
            .process_runtime
            .processes
            .get(&pid)
            .and_then(|entry| entry.lifecycle_sink.clone())
        {
            self.add_supervisor_child(&supervisor_name, pid);
        }
        Ok(pid)
    }

    fn add_supervisor_child(&mut self, supervisor_name: &str, pid: u64) {
        let children = self
            .process_runtime
            .root_supervisor
            .child_table
            .entry(supervisor_name.to_string())
            .or_default();
        if !children.contains(&pid) {
            children.push(pid);
        }
    }

    fn remove_supervisor_child(&mut self, supervisor_name: &str, pid: u64) {
        if let Some(children) = self
            .process_runtime
            .root_supervisor
            .child_table
            .get_mut(supervisor_name)
        {
            children.retain(|child_pid| *child_pid != pid);
        }
    }

    fn is_adoptable_worker(&self, entry: &ProcessInstance) -> bool {
        let Some(spec) = self.process_runtime.spec_for_id(entry.spec_id) else {
            return false;
        };
        matches!(
            entry.status,
            ProcessStatus::Runnable | ProcessStatus::Waiting(_)
        ) && spec.instance == RuntimeProcessInstance::Worker
    }

    fn unique_live_supervisor_child_count(&self, supervisor_name: &str) -> i64 {
        let Some(children) = self
            .process_runtime
            .root_supervisor
            .child_table
            .get(supervisor_name)
        else {
            return 0;
        };
        children
            .iter()
            .filter(|pid| {
                self.process_runtime
                    .processes
                    .get(pid)
                    .is_some_and(|entry| {
                        matches!(
                            entry.status,
                            ProcessStatus::Runnable | ProcessStatus::Waiting(_)
                        )
                    })
            })
            .copied()
            .collect::<BTreeSet<_>>()
            .len() as i64
    }

    pub(crate) fn infer_worker_process_name_from_callable(
        &self,
        callable: &Callable,
    ) -> Option<String> {
        fn worker_process_name(module: &str, name: &str) -> Option<String> {
            matches!(name, "init" | "__agent_init").then(|| module.to_string())
        }

        match (
            callable.metadata.module.as_deref(),
            callable.metadata.name.as_deref(),
        ) {
            (Some(module), Some(name)) if worker_process_name(module, name).is_some() => {
                worker_process_name(module, name)
            }
            _ => {
                let fun_idx =
                    callable
                        .metadata
                        .delegate_function
                        .or_else(|| match callable.target {
                            CallableTarget::Function(fun_idx) => Some(fun_idx),
                            CallableTarget::Template(template_id) => self
                                .callable_template(template_id)
                                .ok()
                                .and_then(|template| match &template.kind {
                                    CallableTemplateKind::PartialDirectCall { target, .. }
                                    | CallableTemplateKind::InjectDirectCall { target, .. } => {
                                        match target {
                                            CallableTemplateDirectTarget::Function(fun_idx) => {
                                                Some(*fun_idx)
                                            }
                                            CallableTemplateDirectTarget::Builtin(_) => None,
                                        }
                                    }
                                    CallableTemplateKind::ComposeDirect { .. } => None,
                                }),
                            CallableTarget::Builtin(_) => None,
                        });
                fun_idx
                    .and_then(|fun_idx| {
                        self.bytecode
                            .functions
                            .get(fun_idx as usize)
                            .and_then(|entry| entry.qualified_name.as_deref())
                            .and_then(|qualified| qualified.rsplit_once("::"))
                            .and_then(|(module, name)| worker_process_name(module, name))
                    })
                    .or_else(|| {
                        callable.lexical_captures.iter().find_map(|value| {
                            let Value::Callable(source) = value else {
                                return None;
                            };
                            self.infer_worker_process_name_from_callable(source)
                        })
                    })
            }
        }
    }

    fn allocate_process_instance(
        &mut self,
        name: String,
        state: Option<Value>,
        owner: Option<u64>,
        lifecycle_sink: Option<LifecycleSink>,
    ) -> Result<u64, RuntimeError> {
        let Some(spec_id) = self.process_runtime.spec_id_by_name.get(&name).copied() else {
            return Err(RuntimeError::new(format!(
                "unknown process spec `{name}` during allocation"
            )));
        };
        let pid = self.process_runtime.next_pid;
        self.process_runtime.next_pid += 1;
        let standby_state_pending = self
            .process_runtime
            .spec_for_id(spec_id)
            .is_some_and(runtime_spec_is_standby)
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
                owner,
                lifecycle_sink,
                standby_state_pending,
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
        let mut stack_trace = self.current_stack_trace_snapshot();
        stack_trace.insert(
            0,
            RuntimeStackFrame {
                phase: RuntimeExecutionPhase::Runtime,
                function: Some("<process>".into()),
                fun_idx: None,
                call_kind: RuntimeCallKind::ProcessMessage,
                location: Some(location.clone()),
                process: Some(RuntimeProcessTraceContext {
                    pid: None,
                    process_name: None,
                    trigger: Some(kind.to_string()),
                }),
                tco: false,
            },
        );
        RichError::new(compiler_global_error_kind(kind), message, location, None)
            .with_stack_trace(stack_trace)
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

    pub(crate) fn process_sleep(&mut self, millis: u64) -> Result<Value, RuntimeError> {
        self.poll_runtime_deadlines();
        if millis == 0 {
            return Ok(ok_vm_result(Value::Unit));
        }
        let future_id = self
            .process_runtime
            .allocate_future_after(None, millis, false);
        Ok(Value::PendingFuture(future_id))
    }

    #[allow(dead_code)]
    fn resolve_sleep_future(&mut self, future_id: FutureId) -> Result<(), RuntimeError> {
        let value = ok_vm_result(Value::Unit);
        let _ = self.process_runtime.resolve_future(future_id, value);
        Ok(())
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
            let cancel_on_timeout = self
                .process_runtime
                .futures
                .get(future_id)
                .is_some_and(|future| future.cancel_on_timeout);
            if cancel_on_timeout {
                self.resolve_future_timeout(*future_id);
            } else {
                let _ = self.resolve_sleep_future(*future_id);
            }
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

    pub fn runtime_output_events_snapshot(&self) -> Vec<VmRuntimeOutputEventSnapshot> {
        self.process_runtime
            .output_events
            .iter()
            .map(RuntimeOutputEvent::snapshot)
            .collect()
    }

    pub fn process_runtime_snapshot(&self) -> VmProcessRuntimeSnapshot {
        let specs = self
            .process_runtime
            .specs_by_id
            .iter()
            .enumerate()
            .map(|(idx, spec)| VmProcessSpecSnapshot {
                spec_id: idx as u32,
                type_name: spec.type_name.clone(),
                kind: format!("{:?}", spec.kind),
                instance: format!("{:?}", spec.instance),
                init_fun_idx: spec.init.callable.fun_idx,
                init_policy: format!("{:?}", spec.init.policy),
                state_type: spec.state.state_type.name.clone(),
                handler_count: spec.handlers.len(),
                dependency_count: spec.dependencies.handlers.len(),
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
                        .map(|spec| spec.type_name.clone())
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
                        standby_state_pending: process.standby_state_pending,
                        state_value: process
                            .state_value
                            .as_ref()
                            .map(|value| crate::builtin::inspect_value(self, value)),
                        execution_context,
                    }
                })
                .collect();
        let worker_sets = self
            .process_runtime
            .worker_sets
            .iter()
            .map(|(id, state)| {
                let live_count = state
                    .members
                    .iter()
                    .filter(|pid| {
                        self.process_runtime
                            .processes
                            .get(pid)
                            .map(|process| {
                                matches!(
                                    process.status,
                                    ProcessStatus::Runnable
                                        | ProcessStatus::Waiting(_)
                                        | ProcessStatus::Restarting
                                )
                            })
                            .unwrap_or(false)
                    })
                    .count();
                VmWorkerSetSnapshot {
                    id: *id,
                    worker_process: state.worker_process.clone(),
                    supervisor: state.supervisor_name.clone(),
                    target: state.target,
                    min: state.strategy.min,
                    max: state.strategy.max,
                    member_pids: state.members.clone(),
                    live_count,
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
            worker_sets,
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

    /// Validate the loaded image and eagerly initialise its runtime boot plan.
    pub(crate) fn boot_runtime(&mut self) -> Result<(), RuntimeError> {
        self.verify_loaded_bytecode()?;
        self.ensure_root_supervisor_booted()
    }

    /// Execute the loaded bytecode (`run` mode expects `Halt`).
    pub fn run(&mut self) -> Result<(), RuntimeError> {
        self.boot_runtime()?;
        self.last_result = None;
        self.test_runner.reset();
        self.test_events.clear();
        self.test_stdout_cursor = self.current_output_len();
        self.test_stderr_cursor = self.current_error_output_len();
        let result = (|| match self.run_until_outcome(self.pc, ExecutionTarget::TopLevel) {
            StepOutcome::Halt(_) => {
                self.last_result = Some(self.stack.last().cloned().unwrap_or(Value::Unit));
                Ok(())
            }
            pending @ StepOutcome::Pending { .. } => match self.drive_pending_to_halt(pending)? {
                StepOutcome::Halt(_) => {
                    self.last_result = Some(self.stack.last().cloned().unwrap_or(Value::Unit));
                    Ok(())
                }
                StepOutcome::RuntimeError(err) => Err(err),
                _ => Err(RuntimeError::new("top-level execution did not finish")),
            },
            StepOutcome::RuntimeError(err) => Err(err),
            StepOutcome::Continue => Err(RuntimeError::new("top-level execution did not finish")),
        })();
        if let Err(error) = &result {
            self.abort_test_case(error);
        }
        if result.is_ok() && self.has_pending_background_work() {
            self.batch_shutdown_pending = true;
        } else {
            self.shutdown_file_resources();
            self.batch_shutdown_pending = false;
        }
        result
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
            type_registry_base: chunk_type_registry_base,
            type_entries,
            error_template_base: chunk_error_template_base,
            error_templates,
            dbg_template_base: chunk_dbg_template_base,
            dbg_templates,
            callable_templates,
            functions,
            docs,
            signatures: _,
            runtime_process_specs,
            runtime_boot_plan,
        } = chunk;
        self.ensure_root_supervisor_booted()?;
        let code_base = self.bytecode.opcodes.len();
        let const_base = self.bytecode.constants.len();
        let type_registry_base = self.bytecode.type_registry.entries().len();
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
        if chunk_type_registry_base as usize != type_registry_base {
            return Err(RuntimeError::new(format!(
                "Chunk type registry base mismatch: chunk={}, vm={}",
                chunk_type_registry_base, type_registry_base
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
        self.bytecode
            .type_registry
            .try_extend(type_entries)
            .map_err(|err| RuntimeError::new(format!("Bytecode verifier: {}", err)))?;
        self.bytecode.error_templates.extend(error_templates);
        self.bytecode.dbg_templates.extend(dbg_templates);
        self.bytecode.callable_templates.extend(callable_templates);
        self.extend_docs_unique(docs);
        self.bytecode
            .runtime_process_specs
            .entries
            .extend(runtime_process_specs);
        self.bytecode
            .runtime_boot_plan
            .singletons
            .extend(runtime_boot_plan.singletons);
        self.bytecode
            .runtime_boot_plan
            .standard_overrides
            .extend(runtime_boot_plan.standard_overrides);
        self.bytecode
            .runtime_boot_plan
            .handler_overrides
            .extend(runtime_boot_plan.handler_overrides);
        self.process_runtime
            .register_spec_table(&self.bytecode.runtime_process_specs);
        self.apply_runtime_supervisor_overrides();
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
                .remove(&spec.type_name);
        }
        self.ensure_root_supervisor_booted()?;

        match self.run_until_outcome(code_base, ExecutionTarget::TopLevel) {
            StepOutcome::Halt(_) => {
                let result = self.stack.pop().unwrap_or(Value::Unit);
                self.last_result = Some(result.clone());
                self.stack.clear();
                Ok(result)
            }
            pending @ StepOutcome::Pending { .. } => match self.drive_pending_to_halt(pending)? {
                StepOutcome::Halt(_) => {
                    let result = self.stack.pop().unwrap_or(Value::Unit);
                    self.last_result = Some(result.clone());
                    self.stack.clear();
                    Ok(result)
                }
                StepOutcome::RuntimeError(err) => Err(err),
                _ => Err(RuntimeError::new("chunk execution did not finish")),
            },
            StepOutcome::RuntimeError(err) => Err(err),
            StepOutcome::Continue => Err(RuntimeError::new("chunk execution did not finish")),
        }
    }

    /// Execute a chunk atomically, preserving the existing VM state on failure.
    pub(crate) fn push_atomic(&mut self, chunk: BytecodeChunk) -> Result<Value, RuntimeError> {
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
            tail_call_breadcrumbs: self.tail_call_breadcrumbs.clone(),
            continuations: self.continuations.clone(),
            pending_invocation: self.pending_invocation.clone(),
            cancellation: self.cancellation.clone(),
            pc: self.pc,
            exit_code: self.exit_code,
            last_result: self.last_result.clone(),
            output_len: self.output.as_ref().map(Vec::len),
            error_output_len: self.error_output.as_ref().map(Vec::len),
            test_runner: self.test_runner.clone(),
            test_event_len: self.test_events.len(),
            test_stdout_cursor: self.test_stdout_cursor,
            test_stderr_cursor: self.test_stderr_cursor,
            stdin_input_cursor: self.stdin_input_cursor,
            process_runtime: self.process_runtime.clone(),
            opcode_len: self.bytecode.opcodes.len(),
            constant_len: self.bytecode.constants.len(),
            type_entry_len: self.bytecode.type_registry.entries().len(),
            error_template_len: self.bytecode.error_templates.len(),
            callable_template_len: self.bytecode.callable_templates.len(),
            function_len: self.bytecode.functions.len(),
            doc_len: self.bytecode.docs.len(),
            process_spec_len: self.bytecode.runtime_process_specs.entries.len(),
            source_map_len: self
                .bytecode
                .source_map
                .as_ref()
                .map(|map| map.entries.len()),
            overwritten_functions,
            open_files: self.open_files.clone(),
            batch_shutdown_pending: self.batch_shutdown_pending,
            next_file_handle_id: self.next_file_handle_id,
            cwd: self.cwd.clone(),
        }
    }

    fn rollback_to_checkpoint(&mut self, checkpoint: VmCheckpoint) {
        self.stack = checkpoint.stack;
        self.frames = checkpoint.frames;
        self.tail_call_breadcrumbs = checkpoint.tail_call_breadcrumbs;
        self.continuations = checkpoint.continuations;
        self.pending_invocation = checkpoint.pending_invocation;
        self.cancellation = checkpoint.cancellation;
        self.pc = checkpoint.pc;
        self.exit_code = checkpoint.exit_code;
        self.last_result = checkpoint.last_result;

        if let (Some(buf), Some(len)) = (self.output.as_mut(), checkpoint.output_len) {
            buf.truncate(len);
        }
        if let (Some(buf), Some(len)) = (self.error_output.as_mut(), checkpoint.error_output_len) {
            buf.truncate(len);
        }
        self.test_runner = checkpoint.test_runner;
        self.test_events.truncate(checkpoint.test_event_len);
        self.test_stdout_cursor = checkpoint.test_stdout_cursor;
        self.test_stderr_cursor = checkpoint.test_stderr_cursor;
        self.stdin_input_cursor = checkpoint.stdin_input_cursor;
        self.process_runtime = checkpoint.process_runtime;
        self.runtime_clock_anchor = Instant::now();
        self.rollback_open_files(checkpoint.open_files);
        self.batch_shutdown_pending = checkpoint.batch_shutdown_pending;
        self.next_file_handle_id = checkpoint.next_file_handle_id;
        self.cwd = checkpoint.cwd;

        self.bytecode.opcodes.truncate(checkpoint.opcode_len);
        self.bytecode.constants.truncate(checkpoint.constant_len);
        self.bytecode
            .type_registry
            .truncate(checkpoint.type_entry_len);
        self.bytecode
            .error_templates
            .truncate(checkpoint.error_template_len);
        self.bytecode
            .callable_templates
            .truncate(checkpoint.callable_template_len);
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

    fn rollback_open_files(&mut self, restored: HashMap<u64, VmOpenFile>) {
        let to_close = self
            .open_files
            .keys()
            .copied()
            .filter(|id| !restored.contains_key(id))
            .collect::<Vec<_>>();
        for handle_id in to_close {
            if let Err(err) = self.close_file_resource(handle_id) {
                self.report_file_shutdown_error(handle_id, &err);
            }
        }
        self.open_files = restored;
    }

    fn shutdown_file_resources(&mut self) {
        let handle_ids = self.open_files.keys().copied().collect::<Vec<_>>();
        for handle_id in handle_ids {
            if let Err(err) = self.close_file_resource(handle_id) {
                self.report_file_shutdown_error(handle_id, &err);
            }
        }
    }

    fn report_file_shutdown_error(&mut self, handle_id: u64, err: &VmFileError) {
        let detail = match err {
            VmFileError::Closed => format!("File shutdown skipped for closed handle #{handle_id}"),
            VmFileError::Io(io_err) => {
                format!("File shutdown failed for handle #{handle_id}: {io_err}")
            }
            VmFileError::Encoding(message) | VmFileError::Message(message) => {
                format!("File shutdown failed for handle #{handle_id}: {message}")
            }
        };
        let _ = self.emit_stderr_text(detail);
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
                "TailCallClosure" => observer.stats.closure_calls += 1,
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

    fn observe_branch_outcome(&mut self, kind: &str, pc: usize, target: u32, taken: bool) {
        if let Some(observer) = self.observer.as_mut() {
            observer.record_branch_outcome(kind, pc, target, taken);
        }
    }

    #[allow(dead_code)]
    fn restore_execution_context(&mut self, context: ProcessExecutionContext) {
        self.stack = context.stack;
        self.frames = context.frames;
        self.tail_call_breadcrumbs = context.tail_call_breadcrumbs;
        self.continuations = context.continuations;
        self.pending_invocation = context.pending_invocation;
        self.cancellation = context.cancellation;
        self.pc = context.pc;
    }

    fn load_local_or_pending(
        &mut self,
        slot: u32,
        resume_pc: usize,
    ) -> Result<OpcodeControl, RuntimeError> {
        let slot_index = slot as usize;
        let value = self
            .current_frame()?
            .locals
            .get(slot_index)
            .cloned()
            .ok_or_else(|| RuntimeError::new(format!("LoadLocal out of bounds: {}", slot)))?;

        match value {
            Value::PendingFuture(future_id) => {
                let resolved = self.ready_future_value(future_id);

                if let Some(value) = resolved {
                    self.current_frame_mut()?.locals[slot_index] = value.clone();
                    self.stack.push(value);
                    Ok(OpcodeControl::Continue)
                } else {
                    Ok(OpcodeControl::Pending {
                        future_id,
                        resume_pc,
                    })
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

    fn resolve_ready_pending_stack_values(&mut self) {
        let ready = self
            .stack
            .iter()
            .filter_map(|value| match value {
                Value::PendingFuture(id) => self.ready_future_value(*id).map(|value| (*id, value)),
                _ => None,
            })
            .collect::<HashMap<_, _>>();
        for value in &mut self.stack {
            if let Value::PendingFuture(id) = value {
                if let Some(resolved) = ready.get(id) {
                    *value = resolved.clone();
                }
            }
        }
    }

    fn take_execution_context(&mut self, pc: usize, target: ExecutionTarget) -> ExecutionContext {
        ExecutionContext {
            stack: std::mem::take(&mut self.stack),
            frames: std::mem::take(&mut self.frames),
            tail_call_breadcrumbs: std::mem::take(&mut self.tail_call_breadcrumbs),
            continuations: std::mem::take(&mut self.continuations),
            pending_invocation: self.pending_invocation.take(),
            cancellation: self.cancellation.take(),
            pc,
            target,
        }
    }

    fn step_context(&mut self, context: &mut ExecutionContext) -> StepOutcome {
        let target = context.target.clone();
        std::mem::swap(&mut self.stack, &mut context.stack);
        std::mem::swap(&mut self.frames, &mut context.frames);
        std::mem::swap(
            &mut self.tail_call_breadcrumbs,
            &mut context.tail_call_breadcrumbs,
        );
        std::mem::swap(&mut self.continuations, &mut context.continuations);
        std::mem::swap(
            &mut self.pending_invocation,
            &mut context.pending_invocation,
        );
        std::mem::swap(&mut self.cancellation, &mut context.cancellation);
        self.pc = context.pc;
        let outcome = self.step_active_context(target.clone());
        match outcome {
            pending @ StepOutcome::Pending { .. } => pending,
            other => {
                *context = self.take_execution_context(self.pc, target);
                other
            }
        }
    }

    fn step_active_context(&mut self, target: ExecutionTarget) -> StepOutcome {
        if self.continuation_ready() {
            return self.step_continuation(target);
        }
        let pc = self.pc;
        if pc >= self.bytecode.opcodes.len() {
            let error = RuntimeError::new("PC out of bounds");
            if self.continuations.is_empty() {
                return StepOutcome::RuntimeError(error);
            }
            self.pending_invocation = Some(Err(self.enrich_continuation_error(error)));
            return StepOutcome::Continue;
        }
        self.resolve_ready_pending_stack_values();
        let current_pc = pc;
        let op = self.bytecode.opcodes[current_pc].clone();
        self.observe_opcode_step(current_pc, &op);
        let mut next_pc = current_pc + 1;
        let control = match self.execute_opcode(op.clone(), &mut next_pc) {
            Ok(control) => control,
            Err(err) => {
                let error = self.enrich_runtime_error(err, current_pc, &op);
                if !self.continuations.is_empty() {
                    self.pending_invocation = Some(Err(error));
                    return StepOutcome::Continue;
                }
                return StepOutcome::RuntimeError(error);
            }
        };
        self.observe_current_depths();

        match control {
            OpcodeControl::Continue => {
                self.pc = next_pc;
                match if self.continuations.is_empty() {
                    self.complete_execution_target(&target)
                } else {
                    Ok(None)
                } {
                    Ok(Some(value)) => StepOutcome::Halt(value),
                    Ok(None) => StepOutcome::Continue,
                    Err(err) => {
                        StepOutcome::RuntimeError(self.enrich_runtime_error(err, current_pc, &op))
                    }
                }
            }
            OpcodeControl::Halt => {
                self.pc = next_pc;
                if !self.continuations.is_empty() {
                    self.pending_invocation = Some(Err(self.enrich_continuation_error(
                        RuntimeError::new("Halt inside a callable continuation"),
                    )));
                    return StepOutcome::Continue;
                }
                StepOutcome::Halt(self.stack.last().cloned().unwrap_or(Value::Unit))
            }
            OpcodeControl::Pending {
                future_id,
                resume_pc,
            } => StepOutcome::Pending {
                future_id,
                resume: self.take_execution_context(resume_pc, target),
            },
        }
    }

    fn run_until_outcome(&mut self, pc: usize, target: ExecutionTarget) -> StepOutcome {
        let mut context = self.take_execution_context(pc, target);
        loop {
            match self.run_quantum(&mut context, &mut Budget::new(128)) {
                ProcessRunOutcome::QuantumExpired => {
                    if let Err(error) = self.drive_ready_runtime_quantum() {
                        Self::cancel_execution_context(&mut context, error);
                    }
                }
                ProcessRunOutcome::Halted(value) => {
                    self.restore_execution_context(context);
                    return StepOutcome::Halt(value);
                }
                ProcessRunOutcome::Pending(future_id) => {
                    return StepOutcome::Pending {
                        future_id,
                        resume: context,
                    }
                }
                ProcessRunOutcome::Failed(error) => {
                    self.restore_execution_context(context);
                    return StepOutcome::RuntimeError(error);
                }
            }
        }
    }

    #[allow(dead_code)]
    fn run_quantum(
        &mut self,
        context: &mut ExecutionContext,
        budget: &mut Budget,
    ) -> ProcessRunOutcome {
        loop {
            if budget.expired() {
                return ProcessRunOutcome::QuantumExpired;
            }
            match self.step_context(context) {
                StepOutcome::Continue => {
                    budget.consume(1);
                    if budget.expired() {
                        return ProcessRunOutcome::QuantumExpired;
                    }
                }
                StepOutcome::Halt(value) => {
                    budget.consume(1);
                    return ProcessRunOutcome::Halted(value);
                }
                StepOutcome::Pending { future_id, resume } => {
                    *context = resume;
                    budget.consume(1);
                    return ProcessRunOutcome::Pending(future_id);
                }
                StepOutcome::RuntimeError(err) => {
                    budget.consume(1);
                    return ProcessRunOutcome::Failed(err);
                }
            }
        }
    }

    #[allow(dead_code)]
    fn scheduler_tick(
        &mut self,
        max_reductions: u64,
    ) -> Result<Option<ProcessRunOutcome>, RuntimeError> {
        let Some(pid) = self.process_runtime.run_queue.pop_front() else {
            return Ok(None);
        };
        let mut context = self
            .process_runtime
            .processes
            .get_mut(&pid)
            .and_then(|process| process.execution_context.take())
            .ok_or_else(|| {
                RuntimeError::new(format!("process {pid} has no execution context to run"))
            })?;
        let mut budget = Budget::new(max_reductions);
        let outcome = self.run_quantum(&mut context, &mut budget);

        match &outcome {
            ProcessRunOutcome::QuantumExpired => {
                if let Some(process) = self.process_runtime.processes.get_mut(&pid) {
                    process.status = ProcessStatus::Runnable;
                    process.execution_context = Some(context);
                }
                self.process_runtime.enqueue_runnable(pid);
            }
            ProcessRunOutcome::Halted(_) => {
                if let Some(process) = self.process_runtime.processes.get_mut(&pid) {
                    process.status = ProcessStatus::Completed;
                    process.execution_context = Some(context);
                }
            }
            ProcessRunOutcome::Pending(future_id) => {
                if let Some(process) = self.process_runtime.processes.get_mut(&pid) {
                    process.execution_context = Some(context);
                }
                self.process_runtime
                    .mark_process_waiting(pid, ProcessWaitReason::Future(*future_id));
            }
            ProcessRunOutcome::Failed(_) => {
                if let Some(process) = self.process_runtime.processes.get_mut(&pid) {
                    process.status = ProcessStatus::Failed;
                    process.execution_context = Some(context);
                }
            }
        }

        Ok(Some(outcome))
    }

    #[allow(dead_code)]
    fn resume_execution(&mut self, context: ProcessExecutionContext) -> StepOutcome {
        let pc = context.pc;
        let target = context.target.clone();
        self.restore_execution_context(context);
        self.run_until_outcome(pc, target)
    }

    fn wait_for_any_future(&mut self, future_ids: &[FutureId]) -> Result<(), RuntimeError> {
        loop {
            self.drive_ready_runtime_quantum()?;
            if future_ids
                .iter()
                .any(|future_id| self.ready_future_value(*future_id).is_some())
            {
                return Ok(());
            }

            if self.has_runnable_background_work() {
                continue;
            }
            let Some(next_deadline) = self.process_runtime.next_running_deadline() else {
                let blocked = future_ids.first().copied().unwrap_or_default();
                return Err(RuntimeError::new(format!(
                    "execution suspended on unresolved future {}",
                    blocked
                )));
            };

            let sleep_ms = next_deadline.saturating_sub(self.process_runtime.current_tick_ms);
            if sleep_ms > 0 {
                std::thread::sleep(Duration::from_millis(sleep_ms));
            }
            self.process_runtime.current_tick_ms = next_deadline;
            self.runtime_clock_anchor = Instant::now();
            self.expire_process_deadlines(next_deadline);
        }
    }

    pub fn has_pending_background_work(&self) -> bool {
        !self.process_runtime.detached_tasks.is_empty()
            || !self.process_runtime.run_queue.is_empty()
            || self.process_runtime.processes.values().any(|process| {
                process.execution_context.is_some()
                    && matches!(process.status, ProcessStatus::Waiting(_))
            })
    }

    pub fn next_background_deadline_delay(&self) -> Option<Duration> {
        if self.has_runnable_background_work() {
            return Some(Duration::ZERO);
        }
        let next_deadline = self.process_runtime.next_running_deadline()?;
        Some(Duration::from_millis(
            next_deadline.saturating_sub(self.process_runtime.current_tick_ms),
        ))
    }

    pub fn pump_background_ready(&mut self) -> Result<(), RuntimeError> {
        self.drive_ready_runtime_quantum()
    }

    pub fn advance_background_time(&mut self, elapsed: Duration) -> Result<(), RuntimeError> {
        self.runtime_clock_anchor = Instant::now();
        let elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        if elapsed_ms == 0 {
            return self.drive_ready_runtime_quantum();
        }

        self.process_runtime.current_tick_ms = self
            .process_runtime
            .current_tick_ms
            .saturating_add(elapsed_ms);
        self.expire_process_deadlines(self.process_runtime.current_tick_ms);
        self.drive_ready_runtime_quantum()
    }

    pub fn pump_background_to_next_deadline(&mut self) -> Result<bool, RuntimeError> {
        if self.has_runnable_background_work() {
            self.drive_ready_runtime_quantum()?;
            return Ok(true);
        }
        let Some(next_deadline) = self.process_runtime.next_running_deadline() else {
            return Ok(false);
        };
        self.process_runtime.current_tick_ms = next_deadline;
        self.runtime_clock_anchor = Instant::now();
        self.expire_process_deadlines(next_deadline);
        self.drive_ready_runtime_quantum()?;
        Ok(true)
    }

    pub fn drain_background_tasks(&mut self) -> Result<(), RuntimeError> {
        while self.has_pending_background_work() {
            self.drive_ready_runtime_quantum()?;
            if !self.has_pending_background_work() {
                break;
            }
            if self.has_runnable_background_work() {
                continue;
            }
            let Some(next_deadline) = self.process_runtime.next_running_deadline() else {
                break;
            };
            let sleep_ms = next_deadline.saturating_sub(self.process_runtime.current_tick_ms);
            if sleep_ms > 0 {
                std::thread::sleep(Duration::from_millis(sleep_ms));
            }
            self.process_runtime.current_tick_ms = next_deadline;
            self.runtime_clock_anchor = Instant::now();
            self.expire_process_deadlines(next_deadline);
        }
        if !self.has_pending_background_work() && self.batch_shutdown_pending {
            self.shutdown_file_resources();
            self.batch_shutdown_pending = false;
        }
        Ok(())
    }

    fn drive_pending_to_halt(
        &mut self,
        mut outcome: StepOutcome,
    ) -> Result<StepOutcome, RuntimeError> {
        loop {
            match outcome {
                StepOutcome::Halt(Value::PendingFuture(future_id)) => {
                    self.wait_for_any_future(&[future_id])?;
                    let value = self.ready_future_value(future_id).ok_or_else(|| {
                        RuntimeError::new(format!("future {} did not resolve", future_id))
                    })?;
                    outcome = StepOutcome::Halt(value);
                }
                StepOutcome::Pending {
                    future_id,
                    mut resume,
                } => {
                    if let Err(error) = self.wait_for_any_future(&[future_id]) {
                        Self::cancel_execution_context(&mut resume, error);
                    }
                    outcome = self.resume_execution(resume);
                }
                other => return Ok(other),
            }
        }
    }

    fn prepare_invocation_context(
        &mut self,
        invocation: Invocation,
    ) -> Result<ExecutionContext, RuntimeError> {
        let saved = self.take_execution_context(self.pc, ExecutionTarget::TopLevel);
        self.frames = saved.frames.clone();
        self.tail_call_breadcrumbs = saved.tail_call_breadcrumbs.clone();
        let site = self.current_frame().ok().and_then(|f| f.call_site);
        let trace = self
            .current_frame()
            .ok()
            .and_then(|f| f.trace_frame.clone());
        let result = self.schedule_invocation(invocation, self.pc, false, site, trace);
        let target = ExecutionTarget::FrameDepth(self.frames.len());
        let context = self.take_execution_context(self.pc, target);
        self.restore_execution_context(saved);
        result.map(|()| context)
    }

    fn prepare_callable_context(
        &mut self,
        callable: Callable,
        args: Vec<Value>,
    ) -> Result<ExecutionContext, RuntimeError> {
        self.prepare_invocation_context(Invocation::Callable(callable, args))
    }

    fn prepare_builtin_context(
        &mut self,
        outcome: BuiltinOutcome,
    ) -> Result<ExecutionContext, RuntimeError> {
        self.prepare_invocation_context(Invocation::Builtin(outcome))
    }

    pub(crate) fn drive_builtin_outcome(
        &mut self,
        outcome: BuiltinOutcome,
    ) -> Result<Value, RuntimeError> {
        let context = self.prepare_builtin_context(outcome)?;
        let saved = self.take_execution_context(self.pc, ExecutionTarget::TopLevel);
        let outcome = self.resume_execution(context);
        let result = match self.drive_pending_to_halt(outcome) {
            Ok(StepOutcome::Halt(value)) => Ok(value),
            Ok(StepOutcome::RuntimeError(error)) | Err(error) => Err(error),
            _ => Err(RuntimeError::new("builtin execution did not finish")),
        };
        self.restore_execution_context(saved);
        result
    }

    #[cfg(test)]
    fn drive_callable_until_wait(&mut self, callable: Callable, args: Vec<Value>) -> StepOutcome {
        match self.prepare_callable_context(callable, args) {
            Ok(context) => self.resume_execution(context),
            Err(error) => StepOutcome::RuntimeError(error),
        }
    }

    pub(crate) fn open_file_resource(
        &mut self,
        path: &str,
        mode: VmFileMode,
    ) -> Result<FileHandleValue, VmFileError> {
        let host_path = self.resolve_host_path(path);
        let file = Self::open_file_for_mode(&host_path, mode).map_err(VmFileError::Io)?;
        let handle = FileHandleValue {
            id: self.next_file_handle_id,
        };
        self.next_file_handle_id += 1;
        self.open_files.insert(
            handle.id,
            VmOpenFile {
                path: host_path.to_string_lossy().into_owned(),
                mode,
                file: Rc::new(RefCell::new(file)),
            },
        );
        Ok(handle)
    }

    pub(crate) fn read_file_chunk(
        &mut self,
        handle_id: u64,
        max_chars: usize,
    ) -> Result<String, VmFileError> {
        let open_file = self
            .open_files
            .get_mut(&handle_id)
            .ok_or(VmFileError::Closed)?;
        if !open_file.mode.can_read() {
            return Err(VmFileError::Message(format!(
                "file handle for {} is not readable in {:?} mode",
                open_file.path, open_file.mode
            )));
        }
        Self::read_utf8_chunk(&mut open_file.file.borrow_mut(), max_chars)
    }

    pub(crate) fn write_file_chunk(
        &mut self,
        handle_id: u64,
        text: &str,
    ) -> Result<(), VmFileError> {
        let open_file = self
            .open_files
            .get_mut(&handle_id)
            .ok_or(VmFileError::Closed)?;
        if !open_file.mode.can_write() {
            return Err(VmFileError::Message(format!(
                "file handle for {} is not writable in {:?} mode",
                open_file.path, open_file.mode
            )));
        }
        open_file
            .file
            .borrow_mut()
            .write_all(text.as_bytes())
            .map_err(VmFileError::Io)
    }

    pub(crate) fn flush_file_resource(&mut self, handle_id: u64) -> Result<(), VmFileError> {
        let open_file = self
            .open_files
            .get_mut(&handle_id)
            .ok_or(VmFileError::Closed)?;
        let result = open_file.file.borrow_mut().flush().map_err(VmFileError::Io);
        result
    }

    pub(crate) fn close_file_resource(&mut self, handle_id: u64) -> Result<(), VmFileError> {
        let Some(open_file) = self.open_files.remove(&handle_id) else {
            return Err(VmFileError::Closed);
        };
        let result = open_file.file.borrow_mut().flush().map_err(VmFileError::Io);
        result
    }

    #[cfg(test)]
    pub(crate) fn open_file_count(&self) -> usize {
        self.open_files.len()
    }

    fn raw_ready_future_value(&self, future_id: FutureId) -> Option<Value> {
        self.process_runtime
            .futures
            .get(&future_id)
            .and_then(|future| match &future.state {
                FutureState::Ready(value) | FutureState::Cancelled(value) => Some(value.clone()),
                FutureState::Running => None,
            })
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

    fn return_from_current_frame(
        &mut self,
        ret: Value,
        pc: &mut usize,
    ) -> Result<(), RuntimeError> {
        if self.frames.len() == 1 {
            return Err(RuntimeError::new("Return at top-level"));
        }

        let frame = self
            .frames
            .pop()
            .ok_or_else(|| RuntimeError::new("Return with empty frame stack"))?;
        self.tail_call_breadcrumbs
            .truncate(frame.tail_call_breadcrumb_base_len);
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
        Ok(())
    }

    fn verify_program(bytecode: &Bytecode) -> Result<(), RuntimeError> {
        validate_type_registry_append_entries(&[], bytecode.type_registry.entries())
            .map_err(|err| RuntimeError::new(format!("Bytecode verifier: {}", err)))?;
        Self::verify_source_map_entries(bytecode.source_map.as_ref(), bytecode.opcodes.len(), "")?;
        validate_program_function_table(
            &bytecode.opcodes,
            &bytecode.callable_templates,
            &bytecode.runtime_process_specs.entries,
            &bytecode.functions,
        )
        .map_err(|err| RuntimeError::new(err.to_string()))?;

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
                Opcode::JumpIfLocalTagEq {
                    tag_const_idx,
                    target_pc: addr,
                    ..
                }
                | Opcode::JumpIfLocalTagNe {
                    tag_const_idx,
                    target_pc: addr,
                    ..
                } => {
                    if *tag_const_idx as usize >= bytecode.constants.len() {
                        return Err(RuntimeError::new(format!(
                            "LoadConst index out of bounds: {}",
                            tag_const_idx
                        )));
                    }
                    if *addr as usize >= bytecode.opcodes.len() {
                        return Err(RuntimeError::new(format!("Invalid jump target: {}", addr)));
                    }
                }
                Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                    if *addr as usize >= bytecode.opcodes.len() {
                        return Err(RuntimeError::new(format!("Invalid jump target: {}", addr)));
                    }
                }
                Opcode::LoadConst(idx)
                | Opcode::StoreConstLocal { const_idx: idx, .. }
                | Opcode::EqLocalTag {
                    tag_const_idx: idx, ..
                } => {
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
                Opcode::LoadBuiltinRef(builtin_id) | Opcode::CallBuiltin { builtin_id, .. } => {
                    if builtin_meta_by_id(*builtin_id).is_none() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: unknown builtin ref {}",
                            builtin_id
                        )));
                    }
                }
                Opcode::LoadFunctionRef(_) | Opcode::Call { .. } => {}
                Opcode::SetCallableDelegateFunction(fun_idx) => {
                    if !bytecode
                        .functions
                        .iter()
                        .any(|entry| entry.fun_idx == *fun_idx)
                    {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: unknown callable delegate function {}",
                            fun_idx
                        )));
                    }
                }
                Opcode::LoadCallableTemplateRef(template_id) => {
                    if !bytecode
                        .callable_templates
                        .iter()
                        .any(|template| template.template_id == *template_id)
                    {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: unknown callable template ref {}",
                            template_id
                        )));
                    }
                }
                Opcode::Dbg { template_id, .. } => {
                    if !bytecode
                        .dbg_templates
                        .iter()
                        .any(|template| template.id == *template_id)
                    {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: unknown dbg template ref {}",
                            template_id
                        )));
                    }
                }
                Opcode::Return | Opcode::TailCallClosure { .. } if idx <= halt_pos => {
                    return Err(RuntimeError::new("Return at top-level"));
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn verify_chunk(&self, chunk: &BytecodeChunk) -> Result<(), RuntimeError> {
        validate_type_registry_append_entries(
            self.bytecode.type_registry.entries(),
            &chunk.type_entries,
        )
        .map_err(|err| RuntimeError::new(format!("Bytecode verifier: {}", err)))?;
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
        let type_registry_base = self.bytecode.type_registry.entries().len();
        if chunk.type_registry_base as usize != type_registry_base {
            return Err(RuntimeError::new(format!(
                "Chunk type registry base mismatch: chunk={}, vm={}",
                chunk.type_registry_base, type_registry_base
            )));
        }

        let halt_pos = chunk
            .opcodes
            .iter()
            .position(|op| matches!(op, Opcode::Halt))
            .ok_or_else(|| RuntimeError::new("Bytecode verifier: chunk missing Halt"))?;

        for (idx, op) in chunk.opcodes.iter().enumerate() {
            match op {
                Opcode::JumpIfLocalTagEq {
                    tag_const_idx,
                    target_pc: addr,
                    ..
                }
                | Opcode::JumpIfLocalTagNe {
                    tag_const_idx,
                    target_pc: addr,
                    ..
                } => {
                    if *tag_const_idx as usize >= chunk.constants.len() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: chunk LoadConst index out of bounds: {}",
                            tag_const_idx
                        )));
                    }
                    if *addr as usize >= chunk.opcodes.len() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: chunk jump target out of bounds: {}",
                            addr
                        )));
                    }
                }
                Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                    if *addr as usize >= chunk.opcodes.len() {
                        return Err(RuntimeError::new(format!(
                            "Bytecode verifier: chunk jump target out of bounds: {}",
                            addr
                        )));
                    }
                }
                Opcode::LoadConst(idx)
                | Opcode::StoreConstLocal { const_idx: idx, .. }
                | Opcode::EqLocalTag {
                    tag_const_idx: idx, ..
                } => {
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
                Opcode::Return | Opcode::TailCallClosure { .. } if idx <= halt_pos => {
                    return Err(RuntimeError::new("Return at top-level"));
                }
                _ => {}
            }
        }

        validate_chunk_function_table(
            &chunk.opcodes,
            &chunk.callable_templates,
            &chunk.runtime_process_specs,
            &chunk.functions,
            self.bytecode.functions.len(),
            true,
        )
        .map_err(|err| RuntimeError::new(err.to_string()))?;

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
                Opcode::JumpIfLocalTagEq {
                    tag_const_idx,
                    target_pc: addr,
                    ..
                }
                | Opcode::JumpIfLocalTagNe {
                    tag_const_idx,
                    target_pc: addr,
                    ..
                } => {
                    *tag_const_idx = tag_const_idx.checked_add(const_base).ok_or_else(|| {
                        RuntimeError::new(format!(
                            "Const relocation overflow: index {} + base {}",
                            *tag_const_idx, const_base
                        ))
                    })?;
                    *addr = addr.checked_add(code_base).ok_or_else(|| {
                        RuntimeError::new(format!(
                            "Jump relocation overflow: target {} + base {}",
                            *addr, code_base
                        ))
                    })?;
                }
                Opcode::Jump(addr) | Opcode::JumpIfFalse(addr) | Opcode::JumpIfTrue(addr) => {
                    *addr = addr.checked_add(code_base).ok_or_else(|| {
                        RuntimeError::new(format!(
                            "Jump relocation overflow: target {} + base {}",
                            *addr, code_base
                        ))
                    })?;
                }
                Opcode::LoadConst(idx)
                | Opcode::StoreConstLocal { const_idx: idx, .. }
                | Opcode::EqLocalTag {
                    tag_const_idx: idx, ..
                } => {
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

        let mut template = template.clone();
        let (file, source) = match sindr::ir::decode_module_source_span(
            template.span_start as usize,
            template.span_end as usize,
        ) {
            Some((source_id, start, end)) => {
                let entry = self
                    .bytecode
                    .sources
                    .iter()
                    .find(|entry| entry.source_id == source_id)
                    .ok_or_else(|| {
                        RuntimeError::new(format!("dbg source {source_id} not found"))
                    })?;
                let source = entry.text.as_deref().ok_or_else(|| {
                    RuntimeError::new(format!("dbg source {source_id} has no text"))
                })?;
                template.span_start = start as u32;
                template.span_end = end as u32;
                for arg in &mut template.args {
                    let (arg_source_id, start, end) = sindr::ir::decode_module_source_span(
                        arg.span_start as usize,
                        arg.span_end as usize,
                    )
                    .ok_or_else(|| RuntimeError::new("dbg argument has no encoded source"))?;
                    if arg_source_id != source_id {
                        return Err(RuntimeError::new(
                            "dbg argument source differs from its expression",
                        ));
                    }
                    arg.span_start = start as u32;
                    arg.span_end = end as u32;
                }
                (
                    entry
                        .normalized_path
                        .as_deref()
                        .unwrap_or(&entry.path)
                        .to_string(),
                    source,
                )
            }
            None => (
                template
                    .source_name
                    .clone()
                    .or_else(|| self.source_file.clone())
                    .unwrap_or_else(|| "<unknown>".into()),
                self.source.as_deref().unwrap_or_default(),
            ),
        };
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

        Ok(render_dbg_report(&file, source, &template, &args))
    }

    fn validate_match_result_representation(
        &self,
        tag: u32,
        fields: &[Value],
    ) -> Result<(), RuntimeError> {
        let Some(entry) = self.type_registry().lookup(tag) else {
            return Ok(());
        };
        let Some(meta) = match_result_variant_meta(&entry.name) else {
            return Ok(());
        };
        if entry.kind != sindr::runtime::TypeKind::EnumVariant
            || fields.len() != 2
            || fields.first() != Some(&Value::Int(int(meta.discriminant)))
            || (meta.carries_error && !matches!(fields.get(1), Some(Value::Error(_))))
        {
            return Err(RuntimeError::new(format!(
                "Invalid MatchResult representation for {}",
                meta.qualified_name
            )));
        }
        Ok(())
    }

    fn execute_opcode(
        &mut self,
        op: Opcode,
        pc: &mut usize,
    ) -> Result<OpcodeControl, RuntimeError> {
        match op {
            Opcode::Halt => return Ok(OpcodeControl::Halt),
            Opcode::Reserved56 => {
                return Err(RuntimeError::new("Reserved opcode tag 56 cannot execute"));
            }

            Opcode::LoadConst(idx) => {
                let val = self.constant_value(idx)?;
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

            Opcode::LoadCallableTemplateRef(template_id) => {
                self.stack.push(Value::Callable(Callable {
                    target: CallableTarget::Template(template_id),
                    lexical_captures: Vec::new(),
                    metadata: self.callable_metadata_for_template(template_id),
                }));
            }

            Opcode::LoadLocal(slot) => {
                return self.load_local_or_pending(slot, (*pc).saturating_sub(1));
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

            Opcode::StoreConstLocal {
                const_idx,
                local_idx,
            } => {
                let val = self.constant_value(const_idx)?;
                let target = self
                    .current_frame_mut()?
                    .locals
                    .get_mut(local_idx as usize)
                    .ok_or_else(|| {
                        RuntimeError::new(format!("StoreConstLocal out of bounds: {}", local_idx))
                    })?;
                *target = val;
            }

            Opcode::CopyLocal {
                src_local_idx,
                dst_local_idx,
            } => {
                let value = self
                    .current_frame()?
                    .locals
                    .get(src_local_idx as usize)
                    .cloned()
                    .ok_or_else(|| {
                        RuntimeError::new(format!(
                            "CopyLocal source out of bounds: {}",
                            src_local_idx
                        ))
                    })?;
                let target = self
                    .current_frame_mut()?
                    .locals
                    .get_mut(dst_local_idx as usize)
                    .ok_or_else(|| {
                        RuntimeError::new(format!(
                            "CopyLocal destination out of bounds: {}",
                            dst_local_idx
                        ))
                    })?;
                *target = value;
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
            Opcode::SafeModInt => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                if b.is_zero() {
                    self.stack.push(err_vm_result(
                        self.process_error("ZeroDivisionError", "division by zero"),
                    ));
                } else {
                    self.stack.push(ok_vm_result(Value::Int(a % b)));
                }
            }
            Opcode::ShlInt => {
                let bits = self.pop_int()?;
                let value = self.pop_int()?;
                let Some(amount) = bits.to_usize() else {
                    self.stack.push(err_vm_result(self.process_error(
                        "NegativeShiftCount",
                        &format!("shift amount must be non-negative: {}", bits),
                    )));
                    return Ok(OpcodeControl::Continue);
                };
                self.stack.push(ok_vm_result(Value::Int(value << amount)));
            }
            Opcode::ShrInt => {
                let bits = self.pop_int()?;
                let value = self.pop_int()?;
                let Some(amount) = bits.to_usize() else {
                    self.stack.push(err_vm_result(self.process_error(
                        "NegativeShiftCount",
                        &format!("shift amount must be non-negative: {}", bits),
                    )));
                    return Ok(OpcodeControl::Continue);
                };
                self.stack.push(ok_vm_result(Value::Int(value >> amount)));
            }
            Opcode::TestBitInt => {
                let index = self.pop_int()?;
                let value = self.pop_int()?;
                if index < int(0) {
                    self.stack.push(err_vm_result(self.process_error(
                        "NegativeBitIndex",
                        &format!("bit index must be non-negative: {}", index),
                    )));
                    return Ok(OpcodeControl::Continue);
                }
                let Some(bit_index) = index.to_usize() else {
                    return Err(RuntimeError::new(format!(
                        "bit index out of range for usize: {}",
                        index
                    )));
                };
                let mask = int(1) << bit_index;
                self.stack
                    .push(ok_vm_result(Value::Bool(!(value & mask).is_zero())));
            }
            Opcode::SetBitInt => {
                let index = self.pop_int()?;
                let value = self.pop_int()?;
                if index < int(0) {
                    self.stack.push(err_vm_result(self.process_error(
                        "NegativeBitIndex",
                        &format!("bit index must be non-negative: {}", index),
                    )));
                    return Ok(OpcodeControl::Continue);
                }
                let Some(bit_index) = index.to_usize() else {
                    return Err(RuntimeError::new(format!(
                        "bit index out of range for usize: {}",
                        index
                    )));
                };
                let mask = int(1) << bit_index;
                self.stack.push(ok_vm_result(Value::Int(value | mask)));
            }
            Opcode::ClearBitInt => {
                let index = self.pop_int()?;
                let value = self.pop_int()?;
                if index < int(0) {
                    self.stack.push(err_vm_result(self.process_error(
                        "NegativeBitIndex",
                        &format!("bit index must be non-negative: {}", index),
                    )));
                    return Ok(OpcodeControl::Continue);
                }
                let Some(bit_index) = index.to_usize() else {
                    return Err(RuntimeError::new(format!(
                        "bit index out of range for usize: {}",
                        index
                    )));
                };
                let mask = int(1) << bit_index;
                self.stack.push(ok_vm_result(Value::Int(value & !mask)));
            }
            Opcode::ToggleBitInt => {
                let index = self.pop_int()?;
                let value = self.pop_int()?;
                if index < int(0) {
                    self.stack.push(err_vm_result(self.process_error(
                        "NegativeBitIndex",
                        &format!("bit index must be non-negative: {}", index),
                    )));
                    return Ok(OpcodeControl::Continue);
                }
                let Some(bit_index) = index.to_usize() else {
                    return Err(RuntimeError::new(format!(
                        "bit index out of range for usize: {}",
                        index
                    )));
                };
                let mask = int(1) << bit_index;
                self.stack.push(ok_vm_result(Value::Int(value ^ mask)));
            }

            // Arithmetic (Float)
            Opcode::AddFloat => self.float_binop("AddFloat", |a, b| a + b)?,
            Opcode::SubFloat => self.float_binop("SubFloat", |a, b| a - b)?,
            Opcode::MulFloat => self.float_binop("MulFloat", |a, b| a * b)?,

            // Comparison (Int)
            Opcode::EqInt => self.int_binop(|a, b| Ok(Value::Bool(a == b)))?,
            Opcode::NeqInt => self.int_binop(|a, b| Ok(Value::Bool(a != b)))?,
            Opcode::LtInt => self.int_binop(|a, b| Ok(Value::Bool(a < b)))?,
            Opcode::GtInt => self.int_binop(|a, b| Ok(Value::Bool(a > b)))?,
            Opcode::LteInt => self.int_binop(|a, b| Ok(Value::Bool(a <= b)))?,
            Opcode::GteInt => self.int_binop(|a, b| Ok(Value::Bool(a >= b)))?,

            // Comparison (Float)
            Opcode::EqFloat => self.float_predicate(|a, b| a == b)?,
            Opcode::NeqFloat => self.float_predicate(|a, b| a != b)?,
            Opcode::LtFloat => self.float_predicate(|a, b| a < b)?,
            Opcode::GtFloat => self.float_predicate(|a, b| a > b)?,
            Opcode::LteFloat => self.float_predicate(|a, b| a <= b)?,
            Opcode::GteFloat => self.float_predicate(|a, b| a >= b)?,

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
            Opcode::EqPid => {
                let right = self.pop_stack()?;
                let left = self.pop_stack()?;
                let (Value::Pid(left), Value::Pid(right)) = (left, right) else {
                    return Err(RuntimeError::new("EqPid requires two PID values"));
                };
                let left_spec = self
                    .process_runtime
                    .spec_by_process_name(&left.process_name)
                    .ok_or_else(|| {
                        RuntimeError::new("EqPid received an unregistered process PID")
                    })?;
                let right_spec = self
                    .process_runtime
                    .spec_by_process_name(&right.process_name)
                    .ok_or_else(|| {
                        RuntimeError::new("EqPid received an unregistered process PID")
                    })?;
                if left_spec.type_name != right_spec.type_name {
                    return Err(RuntimeError::new(
                        "EqPid received PIDs of different process types",
                    ));
                }
                let equal = match left_spec.instance {
                    RuntimeProcessInstance::Singleton => true,
                    RuntimeProcessInstance::Worker => left.id == right.id,
                };
                self.stack.push(Value::Bool(equal));
            }

            // String
            Opcode::ConcatStr => {
                let b = self.pop_str()?;
                let a = self.pop_str()?;
                self.stack.push(Value::Str(a + &b));
            }
            Opcode::StringLen => {
                let value = self.pop_str()?;
                self.stack.push(Value::Int(value.chars().count().into()));
            }
            Opcode::StringContains => {
                let needle = self.pop_str()?;
                let value = self.pop_str()?;
                self.stack.push(Value::Bool(value.contains(&needle)));
            }
            Opcode::StringStartsWith => {
                let prefix = self.pop_str()?;
                let value = self.pop_str()?;
                self.stack.push(Value::Bool(value.starts_with(&prefix)));
            }
            Opcode::StringEndsWith => {
                let suffix = self.pop_str()?;
                let value = self.pop_str()?;
                self.stack.push(Value::Bool(value.ends_with(&suffix)));
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
                self.stack.push(Self::finite_float_value(-a, "NegFloat")?);
            }
            Opcode::NotBool => {
                let a = self.pop_bool()?;
                self.stack.push(Value::Bool(!a));
            }

            // List
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
            Opcode::ListLen => {
                let list = self.pop_stack()?;
                match list {
                    Value::List(handle) => self.stack.push(Value::Int(handle.len().into())),
                    other => {
                        return Err(RuntimeError::new(format!(
                            "ListLen expects List, got {:?}",
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
                    Value::Tagged { tag, fields } => {
                        self.validate_match_result_representation(tag, &fields)?;
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
                    Value::Tagged { tag, fields } => {
                        self.validate_match_result_representation(tag, &fields)?;
                        self.stack.push(Value::Tag(tag));
                    }
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
            Opcode::MakeOk => {
                let payload = self.pop_stack()?;
                self.stack.push(Value::Tagged {
                    tag: 0,
                    fields: vec![payload],
                });
            }
            Opcode::MakeErr => {
                let payload = self.pop_stack()?;
                match payload {
                    Value::Error(_) => {
                        self.stack.push(Value::Tagged {
                            tag: 1,
                            fields: vec![payload],
                        });
                    }
                    other => {
                        return Err(RuntimeError::new(format!(
                            "MakeErr: expected Error, got {:?}",
                            other
                        )));
                    }
                }
            }
            Opcode::EqLocalTag {
                local_idx,
                tag_const_idx,
            } => {
                let expected = self.constant_tag(tag_const_idx)?;
                let actual = self.local_tag(local_idx, "EqLocalTag")?;
                self.stack.push(Value::Bool(actual == expected));
            }
            Opcode::JumpIfLocalTagEq {
                local_idx,
                tag_const_idx,
                target_pc,
            } => {
                let expected = self.constant_tag(tag_const_idx)?;
                if self.local_tag(local_idx, "JumpIfLocalTagEq")? == expected {
                    *pc = self.validate_jump_target(target_pc)?;
                }
            }
            Opcode::JumpIfLocalTagNe {
                local_idx,
                tag_const_idx,
                target_pc,
            } => {
                let expected = self.constant_tag(tag_const_idx)?;
                if self.local_tag(local_idx, "JumpIfLocalTagNe")? != expected {
                    *pc = self.validate_jump_target(target_pc)?;
                }
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
                let trace_frame = self.trace_frame_for_builtin(builtin_name, span_start, span_end);
                self.schedule_callable(
                    Callable {
                        target: CallableTarget::Builtin(builtin_id),
                        lexical_captures: Vec::new(),
                        metadata: Default::default(),
                    },
                    args,
                    *pc,
                    false,
                    Some((span_start, span_end)),
                    Some(trace_frame),
                )?;
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
                    let trace_frame = self.trace_frame_for_call(
                        &entry,
                        RuntimeCallKind::DirectFunction,
                        span_start,
                        span_end,
                        true,
                    );
                    self.record_tail_call_breadcrumb();
                    self.reuse_current_frame_for_call(locals, Some((span_start, span_end)))?;
                    self.current_frame_mut()?.trace_frame = Some(trace_frame);
                    self.observe_tail_call_optimized();
                } else {
                    let return_pc = *pc;
                    let stack_base = self.stack.len();
                    let trace_frame = self.trace_frame_for_call(
                        &entry,
                        RuntimeCallKind::DirectFunction,
                        span_start,
                        span_end,
                        false,
                    );
                    self.frames.push(CallFrame {
                        return_pc,
                        stack_base,
                        call_site: Some((span_start, span_end)),
                        trace_frame: Some(trace_frame),
                        tail_call_breadcrumb_base_len: self.tail_call_breadcrumbs.len(),
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
                let (span_start, span_end) = match template.location_source {
                    sindr::ir::ErrorLocationSource::ConstructorCallSite => {
                        self.current_frame()?.call_site.ok_or_else(|| {
                            RuntimeError::new("Error constructor has no construction source span")
                        })?
                    }
                    sindr::ir::ErrorLocationSource::SourceSpan => {
                        (template.span_start, template.span_end)
                    }
                };
                let mut location = self
                    .location_for_span(span_start, span_end, template.kind.clone())
                    .ok_or_else(|| RuntimeError::new("Error construction span is invalid"))?;
                if location.line == 0
                    && sindr::ir::decode_module_source_span(span_start as usize, span_end as usize)
                        .is_none()
                    && matches!(
                        template.location_source,
                        sindr::ir::ErrorLocationSource::SourceSpan
                    )
                {
                    location.line = template.line;
                    location.column = template.column;
                }
                let mut error = match &template.diagnostic {
                    Some(sindr::ir::RuntimeErrorDiagnosticTemplate::LiteralPatternMismatch {
                        lhs,
                    }) => RichError::new(
                        template.kind.clone(),
                        "Pattern did not match.",
                        location,
                        None,
                    )
                    .with_diagnostic(
                        sindr::runtime::RuntimeErrorDiagnostic::LiteralPatternMismatch {
                            lhs: lhs.clone(),
                            rhs: message,
                        },
                    ),
                    Some(sindr::ir::RuntimeErrorDiagnosticTemplate::SafeBindPatternFailure {
                        rule,
                        input_source,
                    }) => RichError::new(template.kind.clone(), message, location, None)
                        .with_diagnostic(
                            sindr::runtime::RuntimeErrorDiagnostic::SafeBindPatternFailure {
                                rule: rule.clone(),
                                input_source: input_source.clone(),
                            },
                        ),
                    None => RichError::new(template.kind.clone(), message, location, None),
                };
                error = error.with_stack_trace(
                    self.stack_trace_snapshot_without_head_function(&template.kind),
                );
                self.stack.push(Value::Error(Box::new(error)));
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

            Opcode::SetCallableSignature(signature) => {
                let Some(Value::Callable(callable)) = self.stack.last_mut() else {
                    return Err(RuntimeError::new(
                        "SetCallableSignature expects a callable on the stack",
                    ));
                };
                callable.metadata.full_signature = Some(signature.clone());
            }

            Opcode::SetCallableOriginSource(capture_index) => {
                let Some(Value::Callable(callable)) = self.stack.last_mut() else {
                    return Err(RuntimeError::new(
                        "SetCallableOriginSource expects a callable on the stack",
                    ));
                };
                let is_callable_source = matches!(
                    callable.lexical_captures.get(capture_index as usize),
                    Some(Value::Callable(_))
                );
                if !is_callable_source {
                    return Err(RuntimeError::new(format!(
                        "SetCallableOriginSource capture index {} is not a callable",
                        capture_index
                    )));
                }
                callable.metadata.origin_source = Some(capture_index as usize);
            }

            Opcode::SetCallableDelegateFunction(function_index) => {
                let Some(Value::Callable(callable)) = self.stack.last_mut() else {
                    return Err(RuntimeError::new(
                        "SetCallableDelegateFunction expects a callable on the stack",
                    ));
                };
                callable.metadata.delegate_function = Some(function_index);
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

                let lexical_captures = callable.lexical_captures.clone();
                let mut full_args = lexical_captures.clone();
                full_args.extend(args.iter().cloned());

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
                        let trace_frame =
                            self.trace_frame_for_builtin(builtin_name, span_start, span_end);
                        self.schedule_callable(
                            Callable {
                                target: CallableTarget::Builtin(builtin_id),
                                lexical_captures,
                                metadata: Default::default(),
                            },
                            args,
                            *pc,
                            false,
                            Some((span_start, span_end)),
                            Some(trace_frame),
                        )?;
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
                            let trace_frame = self.trace_frame_for_call(
                                &entry,
                                RuntimeCallKind::ClosureFunction,
                                span_start,
                                span_end,
                                true,
                            );
                            self.record_tail_call_breadcrumb();
                            self.reuse_current_frame_for_call(
                                locals,
                                Some((span_start, span_end)),
                            )?;
                            self.current_frame_mut()?.trace_frame = Some(trace_frame);
                            self.observe_tail_call_optimized();
                        } else {
                            let return_pc = *pc;
                            let stack_base = self.stack.len();
                            let trace_frame = self.trace_frame_for_call(
                                &entry,
                                RuntimeCallKind::ClosureFunction,
                                span_start,
                                span_end,
                                false,
                            );
                            self.frames.push(CallFrame {
                                return_pc,
                                stack_base,
                                call_site: Some((span_start, span_end)),
                                trace_frame: Some(trace_frame),
                                tail_call_breadcrumb_base_len: self.tail_call_breadcrumbs.len(),
                                locals,
                            });
                        }
                        *pc = entry.entry_pc as usize;
                    }
                    CallableTarget::Template(template_id) => {
                        self.observe_call_event(
                            "CallClosure",
                            format!(
                                "call pc={} kind=CallClosure target=template:{} arity={} stack_depth={} frame_depth={}",
                                (*pc).saturating_sub(1),
                                template_id,
                                full_args.len(),
                                self.stack.len(),
                                self.frames.len()
                            ),
                        );
                        let trace_frame = self.trace_frame_for_callable_template(
                            template_id,
                            span_start,
                            span_end,
                        );
                        self.schedule_callable(
                            Callable {
                                target: CallableTarget::Template(template_id),
                                lexical_captures,
                                metadata: Default::default(),
                            },
                            args,
                            *pc,
                            false,
                            Some((span_start, span_end)),
                            Some(trace_frame),
                        )?;
                    }
                }
            }

            Opcode::TailCallClosure {
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

                let lexical_captures = callable.lexical_captures.clone();
                let mut full_args = lexical_captures.clone();
                full_args.extend(args.iter().cloned());

                match callable.target {
                    CallableTarget::Builtin(builtin_id) => {
                        let builtin_name = builtin_meta_by_id(builtin_id)
                            .map(|meta| meta.name)
                            .unwrap_or("<unknown>");
                        self.observe_call_event(
                            "TailCallClosure",
                            format!(
                                "call pc={} kind=TailCallClosure target=builtin:{} arity={} stack_depth={} frame_depth={}",
                                (*pc).saturating_sub(1),
                                builtin_name,
                                full_args.len(),
                                self.stack.len(),
                                self.frames.len()
                            ),
                        );
                        let trace_frame =
                            self.trace_frame_for_builtin(builtin_name, span_start, span_end);
                        self.schedule_callable(
                            Callable {
                                target: CallableTarget::Builtin(builtin_id),
                                lexical_captures,
                                metadata: Default::default(),
                            },
                            args,
                            *pc,
                            true,
                            Some((span_start, span_end)),
                            Some(trace_frame),
                        )?;
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
                        self.observe_call_event(
                            "TailCallClosure",
                            format!(
                                "call pc={} kind=TailCallClosure target=function:fun#{} arity={} stack_depth={} frame_depth={}",
                                (*pc).saturating_sub(1),
                                fun_idx,
                                entry.arity,
                                self.stack.len(),
                                self.frames.len()
                            ),
                        );
                        let trace_frame = self.trace_frame_for_call(
                            &entry,
                            RuntimeCallKind::ClosureFunction,
                            span_start,
                            span_end,
                            true,
                        );
                        self.record_tail_call_breadcrumb();
                        self.reuse_current_frame_for_call(locals, Some((span_start, span_end)))?;
                        self.current_frame_mut()?.trace_frame = Some(trace_frame);
                        self.observe_tail_call_optimized();
                        *pc = entry.entry_pc as usize;
                    }
                    CallableTarget::Template(template_id) => {
                        self.observe_call_event(
                            "TailCallClosure",
                            format!(
                                "call pc={} kind=TailCallClosure target=template:{} arity={} stack_depth={} frame_depth={}",
                                (*pc).saturating_sub(1),
                                template_id,
                                full_args.len(),
                                self.stack.len(),
                                self.frames.len()
                            ),
                        );
                        let trace_frame = self.trace_frame_for_callable_template(
                            template_id,
                            span_start,
                            span_end,
                        );
                        self.schedule_callable(
                            Callable {
                                target: CallableTarget::Template(template_id),
                                lexical_captures,
                                metadata: Default::default(),
                            },
                            args,
                            *pc,
                            true,
                            Some((span_start, span_end)),
                            Some(trace_frame),
                        )?;
                    }
                }
            }

            // Control flow
            Opcode::Jump(addr) => {
                *pc = self.validate_jump_target(addr)?;
            }
            Opcode::JumpIfFalse(addr) => {
                let branch_pc = (*pc).saturating_sub(1);
                let val = self.pop_stack()?;
                match val {
                    Value::Bool(false) => {
                        self.observe_branch_outcome("JumpIfFalse", branch_pc, addr, true);
                        *pc = self.validate_jump_target(addr)?;
                    }
                    Value::Bool(true) => {
                        self.observe_branch_outcome("JumpIfFalse", branch_pc, addr, false);
                    }
                    _ => {
                        return Err(RuntimeError::new("JumpIfFalse: expected Bool"));
                    }
                }
            }
            Opcode::JumpIfTrue(addr) => {
                let branch_pc = (*pc).saturating_sub(1);
                let val = self.pop_stack()?;
                match val {
                    Value::Bool(true) => {
                        self.observe_branch_outcome("JumpIfTrue", branch_pc, addr, true);
                        *pc = self.validate_jump_target(addr)?;
                    }
                    Value::Bool(false) => {
                        self.observe_branch_outcome("JumpIfTrue", branch_pc, addr, false);
                    }
                    _ => {
                        return Err(RuntimeError::new("JumpIfTrue: expected Bool"));
                    }
                }
            }

            // Return
            Opcode::Return => {
                let ret = self.pop_stack()?;
                self.return_from_current_frame(ret, pc)?;
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

    pub(super) fn function_entry(&self, fun_idx: u32) -> Result<&FunctionEntry, RuntimeError> {
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

    fn local_tag(&self, local_idx: u32, op_name: &str) -> Result<u32, RuntimeError> {
        match self
            .current_frame()?
            .locals
            .get(local_idx as usize)
            .ok_or_else(|| {
                RuntimeError::new(format!("{op_name} local out of bounds: {local_idx}"))
            })? {
            Value::Tagged { tag, fields } => {
                self.validate_match_result_representation(*tag, fields)?;
                Ok(*tag)
            }
            _ => Err(RuntimeError::new("GetTag on non-tagged value")),
        }
    }

    // Stack helpers

    fn constant_value(&self, idx: u32) -> Result<Value, RuntimeError> {
        let constant =
            self.bytecode.constants.get(idx as usize).ok_or_else(|| {
                RuntimeError::new(format!("LoadConst index out of bounds: {}", idx))
            })?;
        Ok(match constant {
            Constant::Int(n) => Value::Int(n.clone()),
            Constant::Tag(tag) => Value::Tag(*tag),
            Constant::Float(f) => Self::finite_float_value(*f, "LoadConst")?,
            Constant::Str(s) => Value::Str(s.clone()),
            Constant::Bool(b) => Value::Bool(*b),
            Constant::Unit => Value::Unit,
        })
    }

    fn constant_tag(&self, idx: u32) -> Result<u32, RuntimeError> {
        match self.constant_value(idx)? {
            Value::Tag(tag) => Ok(tag),
            other => Err(RuntimeError::new(format!("Expected Tag, got {:?}", other))),
        }
    }

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

    fn open_file_for_mode(path: impl AsRef<Path>, mode: VmFileMode) -> io::Result<File> {
        let mut options = OpenOptions::new();
        match mode {
            VmFileMode::Read => {
                options.read(true);
            }
            VmFileMode::Write => {
                options.write(true).create(true).truncate(true);
            }
            VmFileMode::Append => {
                options.append(true).create(true);
            }
            VmFileMode::ReadWrite => {
                options.read(true).write(true).create(true);
            }
            VmFileMode::ReadAppend => {
                options.read(true).append(true).create(true);
            }
        }
        options.open(path)
    }

    fn read_utf8_chunk(file: &mut File, max_chars: usize) -> Result<String, VmFileError> {
        let mut out = String::new();
        for _ in 0..max_chars {
            let Some(ch) = Self::read_one_utf8_char(file)? else {
                break;
            };
            out.push(ch);
        }
        Ok(out)
    }

    fn read_one_utf8_char(file: &mut File) -> Result<Option<char>, VmFileError> {
        let mut first = [0u8; 1];
        match file.read(&mut first) {
            Ok(0) => return Ok(None),
            Ok(_) => {}
            Err(err) => return Err(VmFileError::Io(err)),
        }

        let width = Self::utf8_char_width(first[0]);
        if width == 0 {
            return Err(VmFileError::Encoding(
                "invalid UTF-8 leading byte while reading file".into(),
            ));
        }

        let mut bytes = vec![first[0]];
        if width > 1 {
            let mut rest = vec![0u8; width - 1];
            file.read_exact(&mut rest).map_err(VmFileError::Io)?;
            bytes.extend(rest);
        }

        let text = std::str::from_utf8(&bytes)
            .map_err(|err| VmFileError::Encoding(format!("invalid UTF-8 sequence: {err}")))?;
        Ok(text.chars().next())
    }

    fn utf8_char_width(first: u8) -> usize {
        match first {
            0x00..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 0,
        }
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
            Value::Float(f) if f.is_finite() => Ok(f),
            Value::Float(_) => Err(RuntimeError::new("Expected finite Float")),
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

    fn finite_float_value(value: f64, op_name: &str) -> Result<Value, RuntimeError> {
        if value.is_finite() {
            Ok(Value::Float(value))
        } else if op_name == "LoadConst" {
            Err(RuntimeError::new("Float constants must be finite"))
        } else {
            Err(RuntimeError::new(format!(
                "Float operation produced non-finite value: {op_name}"
            )))
        }
    }

    fn float_binop<F>(&mut self, op_name: &str, f: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(f64, f64) -> f64,
    {
        let b = self.pop_float()?;
        let a = self.pop_float()?;
        self.stack.push(Self::finite_float_value(f(a, b), op_name)?);
        Ok(())
    }

    fn float_predicate<F>(&mut self, f: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(f64, f64) -> bool,
    {
        let b = self.pop_float()?;
        let a = self.pop_float()?;
        self.stack.push(Value::Bool(f(a, b)));
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

    fn enqueue_runnable(&mut self, pid: u64) {
        if !self.run_queue.contains(&pid) {
            self.run_queue.push_back(pid);
        }
    }

    fn register_spec_table(&mut self, spec_table: &RuntimeProcessSpecTable) {
        self.specs_by_id = spec_table.entries.clone();
        self.spec_id_by_name = spec_table
            .entries
            .iter()
            .enumerate()
            .map(|(idx, spec)| (spec.type_name.clone(), idx as u32))
            .collect();
        self.specs_by_name = spec_table
            .entries
            .iter()
            .cloned()
            .map(|spec| (spec.type_name.clone(), spec))
            .collect();
        let mut effective_supervisors: BTreeMap<String, RuntimeSupervisorPolicy> = spec_table
            .entries
            .iter()
            .filter_map(|spec| {
                spec.supervision
                    .policy
                    .clone()
                    .map(|policy| (spec.type_name.clone(), policy))
            })
            .collect();
        for spec in &spec_table.entries {
            if spec
                .type_name
                .rsplit("::")
                .next()
                .is_some_and(|name| name == "DynamicSupervisor")
            {
                if let Some(policy) = spec.supervision.policy.clone() {
                    effective_supervisors.insert("DynamicSupervisor".into(), policy);
                }
            }
        }
        self.root_supervisor.effective_supervisors = effective_supervisors;
        self.handler_contexts = spec_table
            .entries
            .iter()
            .map(|spec| {
                let slots = spec
                    .dependencies
                    .handlers
                    .iter()
                    .map(|handler| (handler.slot.clone(), handler.default_target.clone()))
                    .collect::<BTreeMap<_, _>>();
                (spec.type_name.clone(), slots)
            })
            .collect();
    }

    fn canonical_process_name<'a>(&'a self, process_name: &'a str) -> Option<&'a str> {
        if self.specs_by_name.contains_key(process_name) {
            return Some(process_name);
        }
        if let Some(surface_name) = process_name.strip_prefix(IMPLICIT_ROOT_NAMESPACE_PREFIX) {
            if self.specs_by_name.contains_key(surface_name) {
                return Some(surface_name);
            }
        } else {
            let canonical_name = format!("{IMPLICIT_ROOT_NAMESPACE_PREFIX}{process_name}");
            if self.specs_by_name.contains_key(&canonical_name) {
                return self
                    .specs_by_name
                    .get_key_value(&canonical_name)
                    .map(|(name, _)| name.as_str());
            }
        }
        None
    }

    fn spec_by_process_name(&self, process_name: &str) -> Option<&RuntimeProcessSpec> {
        self.canonical_process_name(process_name)
            .and_then(|canonical_name| self.specs_by_name.get(canonical_name))
    }

    fn singleton_pid_by_process_name(&self, process_name: &str) -> Option<u64> {
        self.canonical_process_name(process_name)
            .and_then(|canonical_name| self.singleton_by_name.get(canonical_name).copied())
            .or_else(|| self.singleton_by_name.get(process_name).copied())
    }

    fn handler_targets_for_process(
        &self,
        process_name: &str,
    ) -> Option<&BTreeMap<String, RuntimeHandlerTarget>> {
        self.canonical_process_name(process_name)
            .and_then(|canonical_name| self.handler_contexts.get(canonical_name))
            .or_else(|| self.handler_contexts.get(process_name))
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

    fn allocate_future_after(
        &mut self,
        owner: Option<u64>,
        delay_ms: u64,
        cancel_on_timeout: bool,
    ) -> FutureId {
        self.allocate_future(
            owner,
            Some(self.current_tick_ms.saturating_add(delay_ms)),
            cancel_on_timeout,
        )
    }

    fn next_running_deadline(&self) -> Option<u64> {
        self.futures
            .values()
            .filter_map(|future| match future.state {
                FutureState::Running => future.deadline_tick,
                FutureState::Ready(_) | FutureState::Cancelled(_) => None,
            })
            .min()
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
        if !matches!(future.state, FutureState::Running) {
            return Vec::new();
        }
        future.state = FutureState::Ready(value);
        future.deadline_tick = None;
        future.cancel_on_timeout = false;
        if let Some(correlation_id) = future.correlation_id.take() {
            self.reply_table.remove(&correlation_id);
        }
        self.deadline_queue
            .retain(|entry| entry.future_id != future_id);
        self.wake_future_waiters(future_id)
    }

    fn wake_future_waiters(&mut self, future_id: FutureId) -> Vec<u64> {
        let waiters = match self.futures.get_mut(&future_id) {
            Some(future) if !matches!(future.state, FutureState::Running) => {
                std::mem::take(&mut future.waiters)
            }
            _ => return Vec::new(),
        };
        for waiter in &waiters {
            self.waiting_table.remove(waiter);
            let should_enqueue = if let Some(process) = self.processes.get_mut(waiter) {
                process.status = ProcessStatus::Runnable;
                true
            } else {
                false
            };
            if should_enqueue {
                self.enqueue_runnable(*waiter);
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

    fn attach_future_deadline(
        &mut self,
        future_id: FutureId,
        now_tick: u64,
        timeout_ms: u64,
        cancel_on_timeout: bool,
    ) {
        let deadline_tick = now_tick.saturating_add(timeout_ms);
        let Some(future) = self.futures.get_mut(&future_id) else {
            return;
        };
        if !matches!(future.state, FutureState::Running) {
            return;
        }
        future.cancel_on_timeout = future.cancel_on_timeout || cancel_on_timeout;
        let deadline_tick = match future.deadline_tick {
            Some(current) => current.min(deadline_tick),
            None => deadline_tick,
        };
        future.deadline_tick = Some(deadline_tick);
        if !self
            .deadline_queue
            .iter()
            .any(|entry| entry.future_id == future_id && entry.deadline_tick == deadline_tick)
        {
            self.deadline_queue.push_back(DeadlineEntry {
                future_id,
                deadline_tick,
            });
        }
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

fn decode_ok_pid_result(value: Value) -> Option<PidHandle> {
    match value {
        Value::Tagged { tag: 0, fields } if fields.len() == 1 => match fields.first() {
            Some(Value::Pid(pid)) => Some(pid.clone()),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum StandbyInitOutcome {
    Pending,
    PendingAfter(Value),
    Ready(Value),
}

fn decode_standby_init(value: Value) -> Result<StandbyInitOutcome, RuntimeError> {
    match value {
        Value::Tagged { tag: 0, fields } if fields.is_empty() => Ok(StandbyInitOutcome::Pending),
        Value::Tagged { tag: 1, fields } => match fields.as_slice() {
            [duration] => Ok(StandbyInitOutcome::PendingAfter(duration.clone())),
            other => Err(RuntimeError::process_init_failed(format!(
                "StandbyInit::PendingAfter expects one Duration field, got {}",
                other.len()
            ))),
        },
        Value::Tagged { tag: 2, fields } => match fields.as_slice() {
            [state] => Ok(StandbyInitOutcome::Ready(state.clone())),
            other => Err(RuntimeError::process_init_failed(format!(
                "StandbyInit::Ready expects one state field, got {}",
                other.len()
            ))),
        },
        other => Err(RuntimeError::process_init_failed(format!(
            "standby init expects StandbyInit value, got {:?}",
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
    use super::{
        decode_vm_result, ok_vm_result, Budget, CallFrame, ExecutionContext, ExecutionTarget,
        ProcessRunOutcome, ProcessStatus, ProcessWaitReason, RuntimeOutputEvent, StepOutcome,
        TaskMode, VmFileError, VmFileMode, VmObservationOptions, VmRuntimeOutputEventSnapshot, VM,
    };
    use crate::error::RuntimeErrorKind;
    use sindr::ir::{
        BootEntrySource, Bytecode, BytecodeChunk, CallableTemplate, CallableTemplateArg,
        CallableTemplateComposeFlavor, CallableTemplateDirectTarget, CallableTemplateKind,
        Constant, ErrTemplate, FunctionEntry, Opcode, OpcodeSource, RuntimeBootPlan,
        RuntimeCallableRef, RuntimeHandlerKind, RuntimeHandlerSpec, RuntimeInitPolicy,
        RuntimeInitResultShape, RuntimeInitSpec, RuntimeLifecycleSpec, RuntimeProcessDependencies,
        RuntimeProcessInstance, RuntimeProcessKind, RuntimeProcessSpec, RuntimeProcessSpecTable,
        RuntimeStateSpec, RuntimeSupervisionSpec, RuntimeSupervisorOverrideEntry,
        RuntimeSupervisorPolicy, RuntimeTypeRef, SingletonBootEntry, SourceMap,
    };
    use sindr::primitives::int;
    use sindr::runtime::{
        Callable, CallableMetadata, CallableTarget, Location, PidHandle, RichError,
        RuntimeCallKind, TypeEntry, TypeKind, TypeRegistry, Value,
    };
    use std::collections::VecDeque;
    use std::fs;
    use std::path::PathBuf;
    fn test_source_error_template(kind: &str) -> ErrTemplate {
        ErrTemplate {
            id: 0,
            kind: kind.into(),
            location_source: sindr::ir::ErrorLocationSource::SourceSpan,
            span_start: 0,
            span_end: 1,
            line: 1,
            column: 1,
            format: String::new(),
            num_params: 1,
            diagnostic: None,
        }
    }

    pub(super) fn base_bytecode(opcodes: Vec<Opcode>) -> Bytecode {
        Bytecode {
            opcodes,
            type_registry: TypeRegistry::new(),
            ..Bytecode::default()
        }
    }

    fn builtin_id(name: &str) -> u16 {
        sindr::builtin::builtin_function_metas()
            .iter()
            .position(|meta| meta.name == name)
            .unwrap_or_else(|| panic!("missing builtin `{name}`")) as u16
    }

    fn sandbox_dir(prefix: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tmp/sandbox")
            .join(format!("{prefix}-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("sandbox dir should be creatable");
        dir
    }

    pub(super) fn test_runtime_process_spec(
        process_id: u32,
        process_name: impl Into<String>,
        kind: RuntimeProcessKind,
        instance: RuntimeProcessInstance,
        lazy: bool,
        init_fun_idx: u32,
        get_fun_idx: u32,
        set_fun_idx: Option<u32>,
    ) -> RuntimeProcessSpec {
        let process_name = process_name.into();
        let state_type = RuntimeTypeRef { name: "Int".into() };
        let result_type = RuntimeTypeRef {
            name: if lazy {
                "Result<StandbyInit<Int>, Error>".into()
            } else {
                "Result<Int, Error>".into()
            },
        };
        let mut handlers = vec![
            RuntimeHandlerSpec {
                handler_id: 0,
                name: "init".into(),
                kind: RuntimeHandlerKind::Init,
                fun_idx: init_fun_idx,
                arity: 0,
            },
            RuntimeHandlerSpec {
                handler_id: 1,
                name: "get".into(),
                kind: if kind == RuntimeProcessKind::GenServer {
                    RuntimeHandlerKind::Call
                } else {
                    RuntimeHandlerKind::Get
                },
                fun_idx: get_fun_idx,
                arity: 1,
            },
        ];
        if let Some(fun_idx) = set_fun_idx {
            handlers.push(RuntimeHandlerSpec {
                handler_id: 2,
                name: "set".into(),
                kind: if kind == RuntimeProcessKind::GenServer {
                    RuntimeHandlerKind::Cast
                } else {
                    RuntimeHandlerKind::Set
                },
                fun_idx,
                arity: 2,
            });
        }
        RuntimeProcessSpec {
            process_id,
            type_name: process_name.clone(),
            kind,
            instance,
            state: RuntimeStateSpec {
                state_type: state_type.clone(),
            },
            init: RuntimeInitSpec {
                callable: RuntimeCallableRef {
                    fun_idx: init_fun_idx,
                },
                policy: if lazy {
                    RuntimeInitPolicy::Standby
                } else {
                    RuntimeInitPolicy::Eager
                },
                result_shape: if lazy {
                    RuntimeInitResultShape::StandbyProcessInit {
                        result_type: result_type.clone(),
                    }
                } else {
                    RuntimeInitResultShape::EagerState {
                        result_type: result_type.clone(),
                    }
                },
                state_type,
                init_route: None,
            },
            handlers,
            dependencies: RuntimeProcessDependencies::default(),
            lifecycle: RuntimeLifecycleSpec::default(),
            supervision: RuntimeSupervisionSpec::default(),
        }
    }

    pub(super) fn singleton_boot_bytecode(
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
            entries: vec![test_runtime_process_spec(
                0,
                process_name,
                kind,
                RuntimeProcessInstance::Singleton,
                lazy,
                0,
                1,
                None,
            )],
        };
        if boot {
            bytecode.runtime_boot_plan = RuntimeBootPlan::explicit_singleton(process_name);
        }
        bytecode
    }

    fn allow_adopt_policy() -> RuntimeSupervisorPolicy {
        RuntimeSupervisorPolicy {
            strategy: "OneForOne".into(),
            max_restarts: 5,
            max_seconds: 10,
            child_restart_default: "Transient".into(),
            allow_adopt: true,
            shutdown_timeout_ms: None,
        }
    }

    fn root_frame(num_locals: usize) -> CallFrame {
        CallFrame {
            return_pc: 0,
            stack_base: 0,
            call_site: None,
            trace_frame: None,
            tail_call_breadcrumb_base_len: 0,
            locals: vec![Value::Unit; num_locals],
        }
    }

    fn top_level_context(pc: usize, num_locals: usize) -> ExecutionContext {
        ExecutionContext {
            stack: Vec::new(),
            frames: vec![root_frame(num_locals)],
            tail_call_breadcrumbs: VecDeque::new(),
            continuations: Vec::new(),
            pending_invocation: None,
            cancellation: None,
            pc,
            target: ExecutionTarget::TopLevel,
        }
    }

    fn test_process_bytecode(process_name: &str, opcodes: Vec<Opcode>) -> Bytecode {
        let mut bytecode = base_bytecode(opcodes);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                process_name,
                RuntimeProcessKind::Agent,
                RuntimeProcessInstance::Worker,
                false,
                0,
                0,
                None,
            )],
        };
        bytecode
    }

    #[test]
    fn manually_constructed_reserved_opcode_is_rejected() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let error = vm.execute_opcode(Opcode::Reserved56, &mut 0).unwrap_err();
        assert!(error
            .to_string()
            .contains("Reserved opcode tag 56 cannot execute"));
    }

    #[test]
    fn eq_pid_uses_process_instance_identity_and_rejects_invalid_handles() {
        let mut bytecode = base_bytecode(vec![Opcode::EqPid, Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![
                test_runtime_process_spec(
                    0,
                    "Global::One",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Singleton,
                    false,
                    0,
                    0,
                    None,
                ),
                test_runtime_process_spec(
                    1,
                    "Global::Two",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Singleton,
                    false,
                    0,
                    0,
                    None,
                ),
                test_runtime_process_spec(
                    2,
                    "Global::Worker",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Worker,
                    false,
                    0,
                    0,
                    None,
                ),
            ],
        };
        let mut vm = VM::new(bytecode);
        let pid = |name: &str, id| {
            Value::Pid(PidHandle {
                process_name: name.into(),
                id,
            })
        };
        let mut compare = |left: Value, right: Value| {
            vm.stack.push(left);
            vm.stack.push(right);
            let result = vm.execute_opcode(Opcode::EqPid, &mut 0);
            let value = vm.stack.pop();
            (result, value)
        };

        let (result, value) = compare(pid("Global::One", 1), pid("Global::One", 99));
        assert!(result.is_ok());
        assert_eq!(value, Some(Value::Bool(true)));
        let (result, value) = compare(pid("Global::Worker", 7), pid("Global::Worker", 7));
        assert!(result.is_ok());
        assert_eq!(value, Some(Value::Bool(true)));
        let (result, value) = compare(pid("Global::Worker", 7), pid("Global::Worker", 8));
        assert!(result.is_ok());
        assert_eq!(value, Some(Value::Bool(false)));

        assert!(compare(pid("Global::One", 1), pid("Global::Two", 1))
            .0
            .is_err());
        assert!(compare(pid("OutHandler", 1), pid("OutHandler", 1))
            .0
            .is_err());
        assert!(compare(Value::Int(int(1)), pid("Global::Worker", 1))
            .0
            .is_err());
    }

    #[test]
    fn step_context_executes_one_opcode() {
        let mut bytecode = base_bytecode(vec![Opcode::LoadConst(0), Opcode::Halt]);
        bytecode.constants = vec![Constant::Int(int(7))];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);

        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected one opcode to continue, got {other:?}"),
        }

        assert_eq!(ctx.pc, 1);
        assert_eq!(ctx.stack, vec![Value::Int(int(7))]);
        assert_eq!(vm.pc, 1);
    }

    #[test]
    fn store_const_local_executes_as_one_opcode() {
        let mut bytecode = base_bytecode(vec![
            Opcode::StoreConstLocal {
                const_idx: 0,
                local_idx: 1,
            },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(7))];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 2);

        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected one opcode to continue, got {other:?}"),
        }

        assert_eq!(ctx.pc, 1);
        assert_eq!(ctx.stack, Vec::<Value>::new());
        assert_eq!(ctx.frames[0].locals[1], Value::Int(int(7)));
    }

    #[test]
    fn copy_local_executes_as_one_opcode() {
        let bytecode = base_bytecode(vec![
            Opcode::CopyLocal {
                src_local_idx: 0,
                dst_local_idx: 1,
            },
            Opcode::Halt,
        ]);
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 2);
        ctx.frames[0].locals[0] = Value::Int(int(7));

        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected one opcode to continue, got {other:?}"),
        }

        assert_eq!(ctx.pc, 1);
        assert_eq!(ctx.stack, Vec::<Value>::new());
        assert_eq!(ctx.frames[0].locals[1], Value::Int(int(7)));
    }

    #[test]
    fn eq_local_tag_executes_as_one_opcode() {
        let mut bytecode = base_bytecode(vec![
            Opcode::EqLocalTag {
                local_idx: 0,
                tag_const_idx: 0,
            },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Tag(3)];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 1);
        ctx.frames[0].locals[0] = Value::Tagged {
            tag: 3,
            fields: Vec::new(),
        };

        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected one opcode to continue, got {other:?}"),
        }

        assert_eq!(ctx.pc, 1);
        assert_eq!(ctx.stack, vec![Value::Bool(true)]);
    }

    #[test]
    fn make_ok_and_make_err_execute_as_one_opcode() {
        let mut ok_bytecode =
            base_bytecode(vec![Opcode::LoadConst(0), Opcode::MakeOk, Opcode::Halt]);
        ok_bytecode.constants = vec![Constant::Int(int(7))];
        let mut ok_vm = VM::new(ok_bytecode);
        let mut ok_ctx = top_level_context(0, 0);

        match ok_vm.step_context(&mut ok_ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected load const to continue, got {other:?}"),
        }
        match ok_vm.step_context(&mut ok_ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected make ok to continue, got {other:?}"),
        }

        assert_eq!(
            ok_ctx.stack,
            vec![Value::Tagged {
                tag: 0,
                fields: vec![Value::Int(int(7))],
            }]
        );

        let err_bytecode = base_bytecode(vec![Opcode::MakeErr, Opcode::Halt]);
        let mut err_vm = VM::new(err_bytecode);
        let mut err_ctx = top_level_context(0, 0);
        err_ctx.stack.push(Value::Error(Box::new(RichError::new(
            "NoneError",
            "boom",
            Location {
                file: "<test>".into(),
                func: "make_err".into(),
                line: 1,
                column: 1,
                span_start: 0,
                span_end: 0,
            },
            None,
        ))));
        match err_vm.step_context(&mut err_ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected make err to continue, got {other:?}"),
        }

        match &err_ctx.stack[..] {
            [Value::Tagged { tag, fields }] => {
                assert_eq!(*tag, 1);
                assert!(matches!(&fields[..], [Value::Error(_)]));
            }
            other => panic!("expected Err tagged value, got {other:?}"),
        }
    }

    #[test]
    fn make_err_rejects_non_error_payload() {
        let mut bytecode = base_bytecode(vec![Opcode::LoadConst(0), Opcode::MakeErr, Opcode::Halt]);
        bytecode.constants = vec![Constant::Int(int(1))];

        let err = VM::new(bytecode).run().expect_err("must fail");
        assert!(err.message.contains("MakeErr: expected Error"));
    }

    #[test]
    fn result_branch_opcodes_execute_as_one_opcode() {
        let mut bytecode = base_bytecode(vec![
            Opcode::JumpIfLocalTagEq {
                local_idx: 0,
                tag_const_idx: 0,
                target_pc: 1,
            },
            Opcode::JumpIfLocalTagNe {
                local_idx: 1,
                tag_const_idx: 1,
                target_pc: 3,
            },
            Opcode::Halt,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Tag(3), Constant::Tag(4)];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 2);
        ctx.frames[0].locals[0] = Value::Tagged {
            tag: 3,
            fields: Vec::new(),
        };
        ctx.frames[0].locals[1] = Value::Tagged {
            tag: 3,
            fields: Vec::new(),
        };

        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected one opcode to continue, got {other:?}"),
        }
        assert_eq!(ctx.pc, 1);
        assert_eq!(ctx.stack, Vec::<Value>::new());

        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected one opcode to continue, got {other:?}"),
        }
        assert_eq!(ctx.pc, 3);
        assert_eq!(ctx.stack, Vec::<Value>::new());
    }

    #[test]
    fn string_len_executes_as_one_opcode() {
        let mut bytecode =
            base_bytecode(vec![Opcode::LoadConst(0), Opcode::StringLen, Opcode::Halt]);
        bytecode.constants = vec![Constant::Str("あb".into())];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);

        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected load const to continue, got {other:?}"),
        }
        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected string len to continue, got {other:?}"),
        }

        assert_eq!(ctx.pc, 2);
        assert_eq!(ctx.stack, vec![Value::Int(int(2))]);
    }

    #[test]
    fn list_len_executes_as_one_opcode() {
        let bytecode = base_bytecode(vec![Opcode::ListEmpty, Opcode::ListLen, Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);

        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected list empty to continue, got {other:?}"),
        }
        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected list len to continue, got {other:?}"),
        }

        assert_eq!(ctx.pc, 2);
        assert_eq!(ctx.stack, vec![Value::Int(int(0))]);
    }

    #[test]
    fn safe_mod_int_executes_as_one_opcode() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::SafeModInt,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(7)), Constant::Int(int(3))];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);

        assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));
        assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));
        assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));

        assert_eq!(
            ctx.stack,
            vec![Value::Tagged {
                tag: 0,
                fields: vec![Value::Int(int(1))],
            }]
        );
    }

    #[test]
    fn safe_mod_int_returns_zero_division_error_result() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::SafeModInt,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(7)), Constant::Int(int(0))];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);

        assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));
        assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));
        assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));

        match ctx.stack.as_slice() {
            [Value::Tagged { tag: 1, fields }] => match fields.first() {
                Some(Value::Error(rich)) => assert_eq!(rich.kind, "Global::ZeroDivisionError"),
                other => panic!("expected Err(Value::Error), got {other:?}"),
            },
            other => panic!("expected Err result, got {other:?}"),
        }
    }

    #[test]
    fn load_const_rejects_non_finite_float_constants() {
        let mut bytecode = base_bytecode(vec![Opcode::LoadConst(0), Opcode::Halt]);
        bytecode.constants = vec![Constant::Float(f64::INFINITY)];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);

        match vm.step_context(&mut ctx) {
            StepOutcome::RuntimeError(err) => {
                assert!(err.message.contains("Float constants must be finite"))
            }
            other => panic!("expected runtime error, got {other:?}"),
        }
    }

    #[test]
    fn add_float_rejects_non_finite_results() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::AddFloat,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Float(f64::MAX), Constant::Float(f64::MAX)];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);

        assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));
        assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));
        match vm.step_context(&mut ctx) {
            StepOutcome::RuntimeError(err) => {
                assert!(err
                    .message
                    .contains("Float operation produced non-finite value"))
            }
            other => panic!("expected runtime error, got {other:?}"),
        }
    }

    #[test]
    fn string_predicates_execute_as_one_opcode() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::StringContains,
            Opcode::LoadConst(0),
            Opcode::LoadConst(2),
            Opcode::StringStartsWith,
            Opcode::LoadConst(0),
            Opcode::LoadConst(3),
            Opcode::StringEndsWith,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![
            Constant::Str("surtr".into()),
            Constant::Str("urt".into()),
            Constant::Str("sur".into()),
            Constant::Str("tr".into()),
        ];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);

        for _ in 0..9 {
            assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));
        }

        assert_eq!(
            ctx.stack,
            vec![Value::Bool(true), Value::Bool(true), Value::Bool(true)]
        );
    }

    #[test]
    fn step_context_halts_on_halt() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let mut ctx = top_level_context(0, 0);

        match vm.step_context(&mut ctx) {
            StepOutcome::Halt(Value::Unit) => {}
            other => panic!("expected halt unit, got {other:?}"),
        }

        assert_eq!(ctx.pc, 1);
    }

    #[test]
    fn step_context_preserves_runtime_error_context() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::LoadLocal(9), Opcode::Halt]));
        let mut ctx = top_level_context(0, 0);

        match vm.step_context(&mut ctx) {
            StepOutcome::RuntimeError(err) => {
                assert_eq!(err.context.pc, Some(0));
                assert!(err
                    .context
                    .opcode
                    .as_deref()
                    .is_some_and(|opcode| opcode.contains("LoadLocal")));
            }
            other => panic!("expected runtime error, got {other:?}"),
        }
    }

    #[test]
    fn execution_context_round_trips_pending_future_resume() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::LoadLocal(0), Opcode::Return]);
        bytecode.functions = vec![function_entry(0, 1, 1, 1, Some("Main::await_value"))];
        let mut vm = VM::new(bytecode);
        let future_id = vm.process_runtime.allocate_future(None, None, false);
        let callable = Callable {
            target: CallableTarget::Function(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata::default(),
        };

        let resume =
            match vm.drive_callable_until_wait(callable, vec![Value::PendingFuture(future_id)]) {
                StepOutcome::Pending { resume, .. } => resume,
                other => panic!("expected pending outcome, got {other:?}"),
            };
        assert_eq!(resume.pc, 1);

        vm.process_runtime
            .resolve_future(future_id, Value::Int(int(99)));
        match vm.resume_execution(resume) {
            StepOutcome::Halt(Value::Int(value)) => assert_eq!(value, int(99)),
            other => panic!("expected resumed value, got {other:?}"),
        }
    }

    #[test]
    fn run_quantum_expires_on_tail_recursive_loop() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::Call {
                fun_idx: 0,
                arity: 0,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Return,
        ]);
        bytecode.functions = vec![function_entry(0, 1, 0, 0, Some("Main::loop"))];
        let mut vm = VM::new(bytecode);
        let mut ctx = ExecutionContext {
            stack: Vec::new(),
            frames: vec![
                root_frame(0),
                CallFrame {
                    return_pc: usize::MAX,
                    stack_base: 0,
                    call_site: None,
                    trace_frame: None,
                    tail_call_breadcrumb_base_len: 0,
                    locals: Vec::new(),
                },
            ],
            tail_call_breadcrumbs: VecDeque::new(),
            continuations: Vec::new(),
            pending_invocation: None,
            cancellation: None,
            pc: 1,
            target: ExecutionTarget::FrameDepth(1),
        };
        let mut budget = Budget::new(3);

        match vm.run_quantum(&mut ctx, &mut budget) {
            ProcessRunOutcome::QuantumExpired => {}
            other => panic!("expected quantum expiry, got {other:?}"),
        }

        assert_eq!(budget.reductions(), 3);
        assert_eq!(ctx.pc, 1);
    }

    #[test]
    fn run_quantum_resume_continues_after_expiry() {
        let mut bytecode = base_bytecode(vec![Opcode::LoadConst(0), Opcode::Halt]);
        bytecode.constants = vec![Constant::Int(int(42))];
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);
        let mut first_budget = Budget::new(1);

        assert!(matches!(
            vm.run_quantum(&mut ctx, &mut first_budget),
            ProcessRunOutcome::QuantumExpired
        ));
        assert_eq!(ctx.pc, 1);
        assert_eq!(ctx.stack, vec![Value::Int(int(42))]);

        let mut second_budget = Budget::new(1);
        match vm.run_quantum(&mut ctx, &mut second_budget) {
            ProcessRunOutcome::Halted(Value::Int(value)) => assert_eq!(value, int(42)),
            other => panic!("expected halt with stack top, got {other:?}"),
        }
    }

    #[test]
    fn scheduler_requeues_quantum_expired_process() {
        let bytecode = test_process_bytecode("Worker", vec![Opcode::Jump(0), Opcode::Halt]);
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_instance("Worker".into(), Some(Value::Unit), None, None)
            .expect("process allocation should succeed");
        vm.process_runtime
            .processes
            .get_mut(&pid)
            .unwrap()
            .execution_context = Some(top_level_context(0, 0));
        vm.process_runtime.enqueue_runnable(pid);

        assert!(matches!(
            vm.scheduler_tick(1).expect("scheduler tick should run"),
            Some(ProcessRunOutcome::QuantumExpired)
        ));
        assert_eq!(vm.process_runtime.run_queue.front(), Some(&pid));
        assert!(matches!(
            vm.process_runtime.processes.get(&pid).unwrap().status,
            ProcessStatus::Runnable
        ));
    }

    #[test]
    fn scheduler_marks_sleeping_process_waiting_without_blocking_host() {
        let mut bytecode = test_process_bytecode(
            "Sleeper",
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::CallBuiltin {
                    builtin_id: builtin_id("__process_sleep"),
                    arity: 1,
                    span_start: 0,
                    span_end: 0,
                },
                Opcode::Halt,
            ],
        );
        let duration_tag = 20;
        bytecode.type_registry.register(TypeEntry {
            tag: duration_tag,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![false],
        });
        bytecode.constants = vec![Constant::Tag(duration_tag), Constant::Int(int(10))];
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_instance("Sleeper".into(), Some(Value::Unit), None, None)
            .expect("process allocation should succeed");
        vm.process_runtime
            .processes
            .get_mut(&pid)
            .unwrap()
            .execution_context = Some(top_level_context(0, 0));
        vm.process_runtime.enqueue_runnable(pid);

        assert!(matches!(
            vm.scheduler_tick(4).unwrap(),
            Some(ProcessRunOutcome::QuantumExpired)
        ));
        match vm.scheduler_tick(4).expect("scheduler tick should run") {
            Some(ProcessRunOutcome::Pending(future_id)) => {
                assert!(vm.process_runtime.futures.contains_key(&future_id));
            }
            other => panic!("expected sleeping process pending future, got {other:?}"),
        }
        assert!(matches!(
            vm.process_runtime.processes.get(&pid).unwrap().status,
            ProcessStatus::Waiting(ProcessWaitReason::Future(_))
        ));
        assert!(vm.process_runtime.run_queue.is_empty());
    }

    #[test]
    fn due_timer_requeues_sleeping_process_without_host_sleep() {
        let mut bytecode = test_process_bytecode(
            "Sleeper",
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::CallBuiltin {
                    builtin_id: builtin_id("__process_sleep"),
                    arity: 1,
                    span_start: 0,
                    span_end: 0,
                },
                Opcode::Halt,
            ],
        );
        bytecode.type_registry.register(TypeEntry {
            tag: 20,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![false],
        });
        bytecode.constants = vec![Constant::Tag(20), Constant::Int(int(10))];
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_instance("Sleeper".into(), Some(Value::Unit), None, None)
            .expect("process allocation should succeed");
        vm.process_runtime
            .processes
            .get_mut(&pid)
            .unwrap()
            .execution_context = Some(top_level_context(0, 0));
        vm.process_runtime.enqueue_runnable(pid);

        assert!(matches!(
            vm.scheduler_tick(4).unwrap(),
            Some(ProcessRunOutcome::QuantumExpired)
        ));
        assert!(matches!(
            vm.scheduler_tick(4).expect("scheduler tick should run"),
            Some(ProcessRunOutcome::Pending(_))
        ));
        let expired = vm.expire_process_deadlines(10);

        assert_eq!(expired.len(), 1);
        assert_eq!(vm.process_runtime.run_queue.front(), Some(&pid));
        assert!(matches!(
            vm.process_runtime.processes.get(&pid).unwrap().status,
            ProcessStatus::Runnable
        ));
    }

    #[test]
    fn scheduler_preserves_stack_and_frames_per_process() {
        let mut bytecode = test_process_bytecode(
            "Worker",
            vec![Opcode::LoadConst(0), Opcode::LoadConst(1), Opcode::Halt],
        );
        bytecode.constants = vec![Constant::Int(int(1)), Constant::Int(int(2))];
        let mut vm = VM::new(bytecode);
        let first = vm
            .allocate_process_instance("Worker".into(), Some(Value::Unit), None, None)
            .expect("first process allocation should succeed");
        let second = vm
            .allocate_process_instance("Worker".into(), Some(Value::Unit), None, None)
            .expect("second process allocation should succeed");
        vm.process_runtime
            .processes
            .get_mut(&first)
            .unwrap()
            .execution_context = Some(top_level_context(0, 0));
        vm.process_runtime
            .processes
            .get_mut(&second)
            .unwrap()
            .execution_context = Some(top_level_context(1, 0));
        vm.process_runtime.enqueue_runnable(first);
        vm.process_runtime.enqueue_runnable(second);

        assert!(matches!(
            vm.scheduler_tick(1).expect("first tick should run"),
            Some(ProcessRunOutcome::QuantumExpired)
        ));
        assert!(matches!(
            vm.scheduler_tick(1).expect("second tick should run"),
            Some(ProcessRunOutcome::QuantumExpired)
        ));

        let first_stack = &vm
            .process_runtime
            .processes
            .get(&first)
            .unwrap()
            .execution_context
            .as_ref()
            .unwrap()
            .stack;
        let second_stack = &vm
            .process_runtime
            .processes
            .get(&second)
            .unwrap()
            .execution_context
            .as_ref()
            .unwrap()
            .stack;
        assert_eq!(first_stack, &vec![Value::Int(int(1))]);
        assert_eq!(second_stack, &vec![Value::Int(int(2))]);
    }

    #[test]
    fn print_enqueues_runtime_output_event_and_preserves_capture() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt])).with_output_capture();

        vm.emit_stdout_line("hello".into());

        assert_eq!(vm.captured_stdout(), Some(["hello".to_string()].as_slice()));
        assert_eq!(
            vm.process_runtime.output_events.pop_front(),
            Some(RuntimeOutputEvent::StdOut("hello\n".into()))
        );
    }

    #[test]
    fn runtime_output_events_snapshot_preserves_streams_and_order() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]))
            .with_output_capture()
            .with_error_capture();

        vm.emit_stdout_text("out".into())
            .expect("stdout emit should succeed");
        vm.emit_stderr_text("err".into())
            .expect("stderr emit should succeed");

        assert_eq!(
            vm.runtime_output_events_snapshot(),
            vec![
                VmRuntimeOutputEventSnapshot {
                    stream: "stdout".into(),
                    text: "out".into(),
                },
                VmRuntimeOutputEventSnapshot {
                    stream: "stderr".into(),
                    text: "err".into(),
                },
            ]
        );
    }

    #[test]
    fn out_handler_write_records_runtime_output_events() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]))
            .with_output_capture()
            .with_error_capture();

        vm.out_handler_write(&handler_pid("StdOut"), "stdout-handler".into())
            .expect("stdout handler should run");
        vm.out_handler_write(&handler_pid("StdErr"), "stderr-handler".into())
            .expect("stderr handler should run");

        assert_eq!(
            vm.runtime_output_events_snapshot(),
            vec![
                VmRuntimeOutputEventSnapshot {
                    stream: "stdout".into(),
                    text: "stdout-handler".into(),
                },
                VmRuntimeOutputEventSnapshot {
                    stream: "stderr".into(),
                    text: "stderr-handler".into(),
                },
            ]
        );
    }

    #[test]
    fn runtime_output_counter_tracks_output_events() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        vm.enable_observation(VmObservationOptions::default());

        vm.emit_stdout_text("one".into())
            .expect("stdout emit should succeed");
        vm.emit_stderr_text("two".into())
            .expect("stderr emit should succeed");

        assert_eq!(
            vm.observation()
                .expect("observation should be enabled")
                .stats
                .process
                .runtime_output_event_count,
            2
        );
    }

    #[test]
    fn push_atomic_rolls_back_runtime_output_events() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt])).with_output_capture();
        let chunk = BytecodeChunk {
            opcodes: vec![
                Opcode::LoadConst(0),
                Opcode::CallBuiltin {
                    builtin_id: builtin_id("print"),
                    arity: 1,
                    span_start: 0,
                    span_end: 0,
                },
                Opcode::LoadLocal(9),
                Opcode::Halt,
            ],
            source_map: None,
            const_base: 0,
            constants: vec![Constant::Str("rolled back".into())],
            new_locals: 0,
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
        };

        let err = vm.push_atomic(chunk).expect_err("chunk should fail");

        assert!(err.message.contains("LoadLocal out of bounds"));
        assert!(vm.runtime_output_events_snapshot().is_empty());
        assert_eq!(vm.captured_stdout(), Some([].as_slice()));
    }

    #[test]
    fn supervisor_overrides_survive_context_split() {
        let mut bytecode = base_bytecode(vec![Opcode::LoadConst(0), Opcode::Halt]);
        bytecode.constants = vec![Constant::Int(int(1))];
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![supervisor_spec(0, "MySup")],
        };
        bytecode
            .runtime_boot_plan
            .supervisor_overrides
            .push(RuntimeSupervisorOverrideEntry {
                process_name: "MySup".into(),
                policy: RuntimeSupervisorPolicy {
                    allow_adopt: false,
                    max_restarts: 77,
                    ..allow_adopt_policy()
                },
            });
        let mut vm = VM::new(bytecode);
        let mut ctx = top_level_context(0, 0);

        assert!(matches!(vm.step_context(&mut ctx), StepOutcome::Continue));

        let policy = vm
            .process_runtime
            .root_supervisor
            .effective_supervisors
            .get("MySup")
            .expect("supervisor override should remain runtime-global");
        assert!(!policy.allow_adopt);
        assert_eq!(policy.max_restarts, 77);
    }

    #[test]
    fn dynamic_supervisor_policy_remains_available_without_singleton_boot() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![supervisor_spec(0, "Global::DynamicSupervisor")],
        };
        bytecode
            .runtime_boot_plan
            .supervisor_overrides
            .push(RuntimeSupervisorOverrideEntry {
                process_name: "DynamicSupervisor".into(),
                policy: RuntimeSupervisorPolicy {
                    max_restarts: 33,
                    ..allow_adopt_policy()
                },
            });
        let vm = VM::new(bytecode);

        assert_eq!(
            vm.process_runtime
                .root_supervisor
                .effective_supervisors
                .get("DynamicSupervisor")
                .map(|policy| policy.max_restarts),
            Some(33)
        );
        assert!(vm.process_runtime.singleton_by_name.is_empty());
    }

    fn supervisor_spec(process_id: u32, process_name: &str) -> RuntimeProcessSpec {
        RuntimeProcessSpec {
            supervision: RuntimeSupervisionSpec {
                policy: Some(allow_adopt_policy()),
                ..RuntimeSupervisionSpec::default()
            },
            ..test_runtime_process_spec(
                process_id,
                process_name,
                RuntimeProcessKind::Supervisor,
                RuntimeProcessInstance::Singleton,
                false,
                0,
                1,
                None,
            )
        }
    }

    fn supervisor_status_type_registry() -> TypeRegistry {
        let mut registry = TypeRegistry::new();
        registry.register(TypeEntry {
            tag: 10,
            name: "SupervisorStatus".into(),
            kind: TypeKind::Struct,
            field_names: vec![
                "name".into(),
                "child_count".into(),
                "strategy".into(),
                "max_restarts".into(),
                "max_seconds".into(),
                "allow_adopt".into(),
                "shutdown_timeout".into(),
            ],
            private_flags: vec![false; 7],
        });
        registry.register(TypeEntry {
            tag: 11,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![true],
        });
        registry.register(TypeEntry {
            tag: 12,
            name: "Option::None".into(),
            kind: TypeKind::EnumVariant,
            field_names: Vec::new(),
            private_flags: Vec::new(),
        });
        registry.register(TypeEntry {
            tag: 13,
            name: "Option::Some".into(),
            kind: TypeKind::EnumVariant,
            field_names: vec!["value".into()],
            private_flags: vec![false],
        });
        registry
    }

    #[test]
    fn vm_new_registers_runtime_process_specs_from_bytecode() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                "Counter",
                RuntimeProcessKind::Agent,
                RuntimeProcessInstance::Singleton,
                false,
                0,
                1,
                Some(2),
            )],
        };

        let vm = VM::new(bytecode);
        let spec = vm
            .process_runtime
            .specs_by_name
            .get("Counter")
            .expect("process spec should be registered");
        assert_eq!(spec.type_name, "Counter");
        assert_eq!(spec.init.callable.fun_idx, 0);
        assert_eq!(spec.handlers[1].fun_idx, 1);
        assert_eq!(spec.handlers[2].fun_idx, 2);
        assert_eq!(vm.process_runtime.spec_id_by_name.get("Counter"), Some(&0));
        assert_eq!(
            vm.process_runtime
                .spec_for_id(0)
                .expect("spec id 0 should resolve")
                .type_name,
            "Counter"
        );
    }

    #[test]
    fn allocate_process_creates_process_instance_with_runtime_shape() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                "Counter",
                RuntimeProcessKind::Agent,
                RuntimeProcessInstance::Singleton,
                false,
                0,
                1,
                Some(2),
            )],
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
        assert!(!instance.standby_state_pending);
    }

    #[test]
    fn dynamic_supervisor_spawn_records_lifecycle_sink() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                "Worker",
                RuntimeProcessKind::Agent,
                RuntimeProcessInstance::Worker,
                false,
                0,
                1,
                None,
            )],
        };

        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_supervised_worker(
                "Worker".into(),
                Some(Value::Int(int(7))),
                "DynamicSupervisor".into(),
            )
            .expect("supervisor should allocate worker");
        let instance = vm
            .process_runtime
            .processes
            .get(&pid)
            .expect("worker instance should be stored");

        assert_eq!(instance.owner, None);
        assert_eq!(
            instance.lifecycle_sink,
            Some(super::LifecycleSink::Supervisor("DynamicSupervisor".into()))
        );
    }

    #[test]
    fn process_spawn_registers_top_level_worker_under_dynamic_supervisor() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Tag(0), Constant::Int(int(7))];
        bytecode.functions = vec![function_entry(0, 1, 0, 0, Some("Worker::init"))];
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                "Worker",
                RuntimeProcessKind::Agent,
                RuntimeProcessInstance::Worker,
                false,
                0,
                1,
                None,
            )],
        };

        let mut vm = VM::new(bytecode);
        let value = vm
            .process_spawn(
                "Worker".into(),
                Callable {
                    target: CallableTarget::Function(0),
                    lexical_captures: Vec::new(),
                    metadata: CallableMetadata::default(),
                },
            )
            .expect("spawn should return a Result value");
        let pid = match decode_vm_result(value, "test", "spawn") {
            Ok(Ok(Value::Pid(pid))) => pid,
            other => panic!("expected Ok(pid), got {other:?}"),
        };
        let instance = vm
            .process_runtime
            .processes
            .get(&pid.id)
            .expect("spawned worker should be stored");

        assert_eq!(instance.owner, None);
        assert_eq!(
            instance.lifecycle_sink,
            Some(super::LifecycleSink::Supervisor("DynamicSupervisor".into()))
        );
        assert_eq!(
            vm.process_runtime
                .root_supervisor
                .child_table
                .get("DynamicSupervisor"),
            Some(&vec![pid.id])
        );
    }

    #[test]
    fn supervisor_adopt_reassigns_lifecycle_sink() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![
                supervisor_spec(0, "MySup"),
                test_runtime_process_spec(
                    1,
                    "Worker",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Worker,
                    false,
                    0,
                    1,
                    None,
                ),
            ],
        };
        bytecode
            .runtime_boot_plan
            .supervisor_overrides
            .push(RuntimeSupervisorOverrideEntry {
                process_name: "MySup".into(),
                policy: allow_adopt_policy(),
            });
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_state("Worker".into(), Some(Value::Int(int(7))))
            .expect("worker should allocate");
        let value = vm
            .supervisor_adopt(
                "MySup".into(),
                PidHandle {
                    id: pid,
                    process_name: "Worker".into(),
                },
            )
            .expect("adopt should succeed");
        assert!(matches!(
            decode_vm_result(value, "test", "adopt"),
            Ok(Ok(Value::Unit))
        ));
        let instance = vm
            .process_runtime
            .processes
            .get(&pid)
            .expect("worker instance");
        assert_eq!(
            instance.lifecycle_sink,
            Some(super::LifecycleSink::Supervisor("MySup".into()))
        );
    }

    #[test]
    fn supervisor_adopt_is_idempotent_for_same_supervisor() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![
                supervisor_spec(0, "MySup"),
                test_runtime_process_spec(
                    1,
                    "Worker",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Worker,
                    false,
                    0,
                    1,
                    None,
                ),
            ],
        };
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_state("Worker".into(), Some(Value::Int(int(7))))
            .expect("worker should allocate");
        let handle = PidHandle {
            id: pid,
            process_name: "Worker".into(),
        };

        for _ in 0..2 {
            assert!(matches!(
                decode_vm_result(
                    vm.supervisor_adopt("MySup".into(), handle.clone())
                        .expect("adopt should return Result"),
                    "test",
                    "adopt",
                ),
                Ok(Ok(Value::Unit))
            ));
        }

        assert_eq!(
            vm.process_runtime.root_supervisor.child_table.get("MySup"),
            Some(&vec![pid])
        );
    }

    #[test]
    fn supervisor_adopt_handoff_removes_worker_from_old_supervisor() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![
                supervisor_spec(0, "SupA"),
                supervisor_spec(1, "SupB"),
                test_runtime_process_spec(
                    2,
                    "Worker",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Worker,
                    false,
                    0,
                    1,
                    None,
                ),
            ],
        };
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_supervised_worker("Worker".into(), Some(Value::Int(int(7))), "SupA".into())
            .expect("worker should allocate under SupA");

        let value = vm
            .supervisor_adopt(
                "SupB".into(),
                PidHandle {
                    id: pid,
                    process_name: "Worker".into(),
                },
            )
            .expect("handoff adopt should return Result");

        assert!(matches!(
            decode_vm_result(value, "test", "adopt"),
            Ok(Ok(Value::Unit))
        ));
        assert_eq!(
            vm.process_runtime.root_supervisor.child_table.get("SupA"),
            Some(&Vec::<u64>::new())
        );
        assert_eq!(
            vm.process_runtime.root_supervisor.child_table.get("SupB"),
            Some(&vec![pid])
        );
    }

    #[test]
    fn supervisor_adopt_rejects_singleton_pid_with_process_error_result() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![
                supervisor_spec(0, "MySup"),
                test_runtime_process_spec(
                    1,
                    "Counter",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Singleton,
                    false,
                    0,
                    1,
                    None,
                ),
            ],
        };
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_state("Counter".into(), Some(Value::Int(int(7))))
            .expect("singleton process should allocate");

        let value = vm
            .supervisor_adopt(
                "MySup".into(),
                PidHandle {
                    id: pid,
                    process_name: "Counter".into(),
                },
            )
            .expect("adopt rejection should be encoded as Result::Err");

        assert_err_result(value, "SupervisorAdoptInvalidPid", "only Worker");
    }

    #[test]
    fn supervisor_status_counts_unique_live_children() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry = supervisor_status_type_registry();
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![
                supervisor_spec(0, "MySup"),
                test_runtime_process_spec(
                    1,
                    "Worker",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Worker,
                    false,
                    0,
                    1,
                    None,
                ),
            ],
        };
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_state("Worker".into(), Some(Value::Int(int(7))))
            .expect("worker process should allocate");
        vm.process_runtime
            .root_supervisor
            .child_table
            .insert("MySup".into(), vec![pid, pid, pid + 999]);

        let value = vm
            .supervisor_status("MySup".into())
            .expect("status should return Result");

        match decode_vm_result(value, "test", "status") {
            Ok(Ok(Value::Tagged { fields, .. })) => {
                assert_eq!(fields.get(1), Some(&Value::Int(int(1))));
            }
            other => panic!("expected Ok(SupervisorStatus), got {other:?}"),
        }
    }

    #[test]
    fn supervisor_status_reports_missing_shutdown_timeout_as_option_none() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry = supervisor_status_type_registry();
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![supervisor_spec(0, "MySup")],
        };
        let mut vm = VM::new(bytecode);

        let value = vm
            .supervisor_status("MySup".into())
            .expect("status should return Result");

        match decode_vm_result(value, "test", "status") {
            Ok(Ok(Value::Tagged { fields, .. })) => {
                assert_eq!(
                    fields.get(6),
                    Some(&Value::Tagged {
                        tag: 12,
                        fields: vec![Value::Int(int(1))],
                    })
                );
            }
            other => panic!("expected Ok(SupervisorStatus), got {other:?}"),
        }
    }

    #[test]
    fn supervisor_status_reports_configured_shutdown_timeout_as_option_duration() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry = supervisor_status_type_registry();
        let mut policy = allow_adopt_policy();
        policy.shutdown_timeout_ms = Some(250);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![RuntimeProcessSpec {
                supervision: RuntimeSupervisionSpec {
                    policy: Some(policy),
                    ..RuntimeSupervisionSpec::default()
                },
                ..test_runtime_process_spec(
                    0,
                    "MySup",
                    RuntimeProcessKind::Supervisor,
                    RuntimeProcessInstance::Singleton,
                    false,
                    0,
                    1,
                    None,
                )
            }],
        };
        let mut vm = VM::new(bytecode);

        let value = vm
            .supervisor_status("MySup".into())
            .expect("status should return Result");

        match decode_vm_result(value, "test", "status") {
            Ok(Ok(Value::Tagged { fields, .. })) => {
                assert_eq!(
                    fields.get(6),
                    Some(&Value::Tagged {
                        tag: 13,
                        fields: vec![
                            Value::Int(int(0)),
                            Value::Tagged {
                                tag: 11,
                                fields: vec![Value::Int(int(250))],
                            },
                        ],
                    })
                );
            }
            other => panic!("expected Ok(SupervisorStatus), got {other:?}"),
        }
    }

    #[test]
    fn process_state_and_store_validate_pid_against_registered_spec() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![
                test_runtime_process_spec(
                    0,
                    "Counter",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Singleton,
                    false,
                    0,
                    1,
                    Some(2),
                ),
                test_runtime_process_spec(
                    0,
                    "Clock",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Singleton,
                    false,
                    3,
                    4,
                    Some(5),
                ),
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

    fn handler_pid(identity: &str) -> PidHandle {
        PidHandle {
            id: 0,
            process_name: identity.to_string(),
        }
    }

    fn assert_ok_unit_result(value: Value) {
        assert_eq!(value, super::ok_vm_result(Value::Unit));
    }

    fn assert_err_result(value: Value, kind: &str, message_part: &str) {
        match value {
            Value::Tagged { tag: 1, fields } => match fields.as_slice() {
                [Value::Error(error)] => {
                    assert_eq!(error.kind, sindr::names::compiler_global_error_kind(kind));
                    assert!(
                        error.visible_message().contains(message_part),
                        "expected `{}` in `{}`",
                        message_part,
                        error.visible_message()
                    );
                }
                other => panic!("expected Err(error), got {other:?}"),
            },
            other => panic!("expected Result::Err, got {other:?}"),
        }
    }

    #[test]
    fn out_handler_write_dispatches_to_stdout_stderr_and_null_targets() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]))
            .with_output_capture()
            .with_error_capture();

        let stdout = vm
            .out_handler_write(&handler_pid("StdOut"), "stdout-handler".into())
            .expect("stdout handler should run");
        assert_ok_unit_result(stdout);
        assert_eq!(vm.take_stdout(), vec!["stdout-handler".to_string()]);
        assert_eq!(vm.take_stderr(), Vec::<String>::new());

        let stderr = vm
            .out_handler_write(&handler_pid("StdErr"), "stderr-handler".into())
            .expect("stderr handler should run");
        assert_ok_unit_result(stderr);
        assert_eq!(vm.take_stdout(), Vec::<String>::new());
        assert_eq!(vm.take_stderr(), vec!["stderr-handler".to_string()]);

        let null = vm
            .out_handler_write(&handler_pid("NullOutHandler"), "muted".into())
            .expect("null handler should run");
        assert_ok_unit_result(null);
        assert_eq!(vm.take_stdout(), Vec::<String>::new());
        assert_eq!(vm.take_stderr(), Vec::<String>::new());
    }

    #[test]
    fn repl_host_io_buffering_captures_standard_handlers_without_overriding_other_targets() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        vm.enable_repl_host_io_buffering();

        let stdout = vm
            .out_handler_write(&handler_pid("StdOut"), "stdout-host".into())
            .expect("stdout handler should run");
        assert_ok_unit_result(stdout);
        assert_eq!(vm.take_repl_host_stdout(), vec!["stdout-host".to_string()]);
        assert_eq!(vm.take_repl_host_stderr(), Vec::<String>::new());
        assert_eq!(vm.take_stdout(), Vec::<String>::new());

        let stderr = vm
            .out_handler_write(&handler_pid("StdErr"), "stderr-host".into())
            .expect("stderr handler should run");
        assert_ok_unit_result(stderr);
        assert_eq!(vm.take_repl_host_stdout(), Vec::<String>::new());
        assert_eq!(vm.take_repl_host_stderr(), vec!["stderr-host".to_string()]);
        assert_eq!(vm.take_stderr(), Vec::<String>::new());

        let null = vm
            .out_handler_write(&handler_pid("NullOutHandler"), "muted".into())
            .expect("null handler should run");
        assert_ok_unit_result(null);
        assert_eq!(vm.take_repl_host_stdout(), Vec::<String>::new());
        assert_eq!(vm.take_repl_host_stderr(), Vec::<String>::new());

        let dir = std::env::temp_dir().join(format!(
            "surtr-eldr-repl-host-buffer-file-out-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        let path = dir.join("process.log");
        let _ = std::fs::remove_file(&path);
        let file = vm
            .out_handler_write(
                &handler_pid(&format!("FileOutHandler(path={})", path.display())),
                "file-host\n".into(),
            )
            .expect("file handler should run");
        assert_ok_unit_result(file);
        assert_eq!(vm.take_repl_host_stdout(), Vec::<String>::new());
        assert_eq!(vm.take_repl_host_stderr(), Vec::<String>::new());
        assert_eq!(
            std::fs::read_to_string(&path).expect("file output should exist"),
            "file-host\n"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn out_handler_write_appends_to_file_target() {
        let dir = std::env::temp_dir().join(format!(
            "surtr-eldr-file-out-handler-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        let path = dir.join("process.log");
        let _ = std::fs::remove_file(&path);
        let identity = format!("FileOutHandler(path={})", path.display());

        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]))
            .with_output_capture()
            .with_error_capture();
        let first = vm
            .out_handler_write(&handler_pid(&identity), "one\n".into())
            .expect("file handler should run");
        let second = vm
            .out_handler_write(&handler_pid(&identity), "two\n".into())
            .expect("file handler should append");

        assert_ok_unit_result(first);
        assert_ok_unit_result(second);
        assert_eq!(vm.take_stdout(), Vec::<String>::new());
        assert_eq!(vm.take_stderr(), Vec::<String>::new());
        assert_eq!(
            std::fs::read_to_string(&path).expect("file output should exist"),
            "one\ntwo\n"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn out_handler_write_reports_invalid_file_and_unknown_targets_as_result_errors() {
        let missing_path = std::env::temp_dir()
            .join("surtr-eldr-missing-parent")
            .join("process.log");
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]))
            .with_output_capture()
            .with_error_capture();

        let without_path = vm
            .out_handler_write(&handler_pid("FileOutHandler"), "ignored".into())
            .expect("missing path should be a Result error");
        assert_err_result(
            without_path,
            "HandlerInitFailed",
            "requires named argument `path`",
        );

        let missing_parent = vm
            .out_handler_write(
                &handler_pid(&format!("FileOutHandler(path={})", missing_path.display())),
                "ignored".into(),
            )
            .expect("open failure should be a Result error");
        assert_err_result(missing_parent, "HandlerInitFailed", "open failed");

        let unknown = vm
            .out_handler_write(&handler_pid("BogusOutHandler"), "ignored".into())
            .expect("unknown handler should be a Result error");
        assert_err_result(
            unknown,
            "UnknownHandlerTarget",
            "unknown OutHandler target `BogusOutHandler`",
        );
    }

    #[test]
    fn rollback_to_checkpoint_closes_new_file_resources() {
        let dir = sandbox_dir("vm-file-rollback");
        let path = dir.join("rollback.txt");
        let path_text = path.to_string_lossy().into_owned();
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let chunk = BytecodeChunk {
            opcodes: Vec::new(),
            source_map: None,
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_registry_base: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: RuntimeBootPlan::default(),
        };
        let checkpoint = vm.checkpoint_for_chunk(&chunk);
        let handle = vm
            .open_file_resource(&path_text, VmFileMode::Write)
            .expect("file handle should open");
        assert_eq!(vm.open_file_count(), 1);
        vm.rollback_to_checkpoint(checkpoint);
        assert_eq!(
            vm.open_file_count(),
            0,
            "rollback should close new file handles"
        );
        assert!(
            matches!(vm.flush_file_resource(handle.id), Err(VmFileError::Closed)),
            "rolled back handle should be closed"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn run_shutdown_closes_remaining_file_resources() {
        let dir = sandbox_dir("vm-file-run-shutdown");
        let path = dir.join("run.txt");
        let path_text = path.to_string_lossy().into_owned();
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let handle = vm
            .open_file_resource(&path_text, VmFileMode::Write)
            .expect("file handle should open");
        assert_eq!(vm.open_file_count(), 1);
        vm.run().expect("halt-only bytecode should run");
        assert_eq!(
            vm.open_file_count(),
            0,
            "run should shutdown file resources"
        );
        assert!(
            matches!(vm.flush_file_resource(handle.id), Err(VmFileError::Closed)),
            "run shutdown should close the handle"
        );
        let _ = fs::remove_dir_all(dir);
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
    fn process_sleep_suspends_with_deadline_future_and_resumes_ok() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let value = vm.process_sleep(10).expect("sleep should schedule");
        let Value::PendingFuture(future_id) = value else {
            panic!("expected pending sleep future, got {value:?}");
        };
        let future = vm
            .process_runtime
            .futures
            .get(&future_id)
            .expect("sleep future should be tracked");
        assert_eq!(future.deadline_tick, Some(10));
        assert_eq!(future.state, super::FutureState::Running);
        assert_eq!(
            vm.process_runtime.deadline_queue.front(),
            Some(&super::DeadlineEntry {
                deadline_tick: 10,
                future_id,
            })
        );

        vm.resolve_sleep_future(future_id)
            .expect("sleep future should resolve");
        assert_eq!(
            vm.ready_future_value(future_id),
            Some(super::ok_vm_result(Value::Unit))
        );
    }

    #[test]
    fn common_driver_waits_for_sleep_future_and_returns_ok() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 2,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![true],
        });
        let mut vm = VM::new(bytecode);
        let callable = Callable {
            target: CallableTarget::Builtin(builtin_id("__process_sleep")),
            lexical_captures: vec![Value::Tagged {
                tag: 2,
                fields: vec![Value::Int(int(20))],
            }],
            metadata: CallableMetadata::default(),
        };

        let value = vm
            .drive_builtin_outcome(crate::builtin::BuiltinOutcome::Call {
                callable,
                args: Vec::new(),
                continuation: crate::builtin::BuiltinContinuation::Identity,
            })
            .expect("sync call should await sleep");

        assert_eq!(value, ok_vm_result(Value::Unit));
        assert_eq!(vm.process_runtime.current_tick_ms, 20);
    }

    #[test]
    fn task_async_returns_handle_and_await_completes_after_sleep() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 2,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![true],
        });
        let mut vm = VM::new(bytecode);
        let callable = Callable {
            target: CallableTarget::Builtin(builtin_id("__process_sleep")),
            lexical_captures: vec![Value::Tagged {
                tag: 2,
                fields: vec![Value::Int(int(20))],
            }],
            metadata: CallableMetadata::default(),
        };

        let task = vm
            .invoke_task(callable, TaskMode::Async)
            .expect("task async should return a handle");

        assert!(matches!(task, Value::TaskHandle(_)));
        let value = vm
            .await_task_handle(&task, None)
            .expect("awaiting task handle should finish");
        assert_eq!(value, ok_vm_result(Value::Unit));
        assert_eq!(vm.process_runtime.current_tick_ms, 20);
    }

    #[test]
    fn task_awaited_err_keeps_task_body_and_await_trace() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(2),
            Opcode::MakeError { template_id: 0 },
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
        ]);
        bytecode.constants = vec![
            Constant::Tag(1),
            Constant::Str("Boom".into()),
            Constant::Str("task failed".into()),
        ];
        bytecode.functions = vec![function_entry(0, 1, 0, 0, Some("Main::task_body"))];
        bytecode.error_templates = vec![test_source_error_template("Boom")];
        let mut vm =
            VM::new(bytecode).with_source("Task::async(body)\n".into(), "sample.srt".into());
        vm.frames[0].call_site = Some((0, 17));
        let callable = vm.callable_for_function(0);

        let task = vm
            .invoke_task(callable, TaskMode::Async)
            .expect("task async should return a handle");
        let value = vm
            .await_task_handle(&task, None)
            .expect("awaiting task handle should finish");

        let Value::Tagged { tag: 1, fields } = value else {
            panic!("task should resolve Err(Error)");
        };
        let Some(Value::Error(rich)) = fields.first() else {
            panic!("task Err should contain RichError");
        };
        let frame_names: Vec<_> = rich
            .stack_trace
            .iter()
            .filter_map(|frame| frame.function.as_deref())
            .collect();
        assert_eq!(frame_names, vec!["Task::await", "task_body", "Task::async"]);
        assert_eq!(
            rich.stack_trace
                .iter()
                .map(|frame| frame.call_kind.clone())
                .collect::<Vec<_>>(),
            vec![
                RuntimeCallKind::Task,
                RuntimeCallKind::ClosureFunction,
                RuntimeCallKind::Task
            ]
        );
    }

    #[test]
    fn task_launch_returns_before_sleep_future_resolves() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::StructNew { field_count: 1 },
            Opcode::CallBuiltin {
                builtin_id: builtin_id("__process_sleep"),
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Return,
        ]);
        bytecode.type_registry.register(TypeEntry {
            tag: 2,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![true],
        });
        bytecode.constants = vec![Constant::Tag(2), Constant::Int(int(20))];
        bytecode.functions = vec![function_entry(0, 1, 0, 0, Some("Main::sleep_then_return"))];
        let mut vm = VM::new(bytecode);
        let callable = Callable {
            target: CallableTarget::Function(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata::default(),
        };

        let value = vm
            .invoke_task(callable, TaskMode::Launch)
            .expect("task launch should accept sleep body");

        assert_eq!(value, ok_vm_result(Value::Unit));
        assert_eq!(vm.process_runtime.futures.len(), 1);
        assert_eq!(
            vm.process_runtime
                .futures
                .values()
                .filter(|future| matches!(future.state, super::FutureState::Running))
                .count(),
            1
        );
        assert_eq!(vm.process_runtime.detached_tasks.len(), 1);

        vm.process_runtime.current_tick_ms = 20;
        let expired = vm.expire_process_deadlines(20);
        assert_eq!(expired.len(), 1);
        vm.drive_ready_detached_tasks()
            .expect("detached task should resume cleanly");
        assert!(vm.process_runtime.detached_tasks.is_empty());
    }

    #[test]
    fn task_timeout_deadline_wins_before_unresolved_sleep_completion() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 2,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![true],
        });
        let mut vm = VM::new(bytecode);
        let callable = Callable {
            target: CallableTarget::Builtin(builtin_id("__process_sleep")),
            lexical_captures: vec![Value::Tagged {
                tag: 2,
                fields: vec![Value::Int(int(100))],
            }],
            metadata: CallableMetadata::default(),
        };

        let task = vm
            .invoke_task_with_timeout(callable, TaskMode::Async, Some(1))
            .expect("task async with timeout should return a handle");
        assert!(matches!(task, Value::TaskHandle(_)));
        let value = vm
            .await_task_handle(&task, None)
            .expect("awaiting timed task should resolve timeout result");
        assert!(matches!(
            value,
            Value::Tagged { tag: 1, fields } if matches!(fields.first(), Some(Value::Error(err)) if err.kind == "Global::Timeout")
        ));
        assert!(
            vm.process_runtime.reply_table.is_empty(),
            "timeout should not leave reply waiters"
        );
        assert!(
            vm.process_runtime
                .deadline_queue
                .iter()
                .all(|entry| entry.deadline_tick > 1),
            "completion timeout deadline should be consumed"
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
            entries: vec![test_runtime_process_spec(
                0,
                "Counter",
                RuntimeProcessKind::Agent,
                RuntimeProcessInstance::Singleton,
                false,
                0,
                1,
                Some(2),
            )],
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
            entries: vec![test_runtime_process_spec(
                0,
                "Counter",
                RuntimeProcessKind::Agent,
                RuntimeProcessInstance::Singleton,
                false,
                0,
                1,
                Some(2),
            )],
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
        assert!(vm
            .process_runtime
            .deadline_queue
            .iter()
            .all(|entry| entry.future_id != future_id));
        assert_eq!(vm.process_runtime.waiting_table.get(&pid), None);
        let future = vm
            .process_runtime
            .futures
            .get(&future_id)
            .expect("future should exist");
        assert_eq!(future.deadline_tick, None);
        assert!(matches!(
            future.state,
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            RuntimeProcessKind::Agent,
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
        assert!(!instance.standby_state_pending);
    }

    #[test]
    fn root_supervisor_uses_boot_plan_singletons() {
        let mut bytecode = singleton_boot_bytecode(
            "Counter",
            RuntimeProcessKind::Agent,
            false,
            false,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![Constant::Tag(0), Constant::Int(int(41))],
        );
        bytecode.runtime_boot_plan = RuntimeBootPlan::explicit_singleton("Counter");
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
    }

    #[test]
    fn root_supervisor_rejects_unknown_boot_plan_singleton() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_boot_plan = RuntimeBootPlan::explicit_singleton("Missing");
        let mut vm = VM::new(bytecode);

        let err = vm
            .ensure_root_supervisor_booted()
            .expect_err("unknown singleton boot entry should fail");

        assert!(err.message.contains("not defined or not visible"));
    }

    #[test]
    fn root_supervisor_rejects_worker_boot_plan_singleton_entry() {
        let mut bytecode = singleton_boot_bytecode(
            "WorkerAgent",
            RuntimeProcessKind::Agent,
            false,
            false,
            vec![Opcode::LoadConst(0), Opcode::Return],
            vec![Constant::Int(int(0))],
        );
        bytecode.runtime_process_specs.entries[0].instance = RuntimeProcessInstance::Worker;
        bytecode.runtime_boot_plan = RuntimeBootPlan::explicit_singleton("WorkerAgent");
        let mut vm = VM::new(bytecode);

        let err = vm
            .ensure_root_supervisor_booted()
            .expect_err("worker singleton boot entry should fail");

        assert!(err.message.contains("only Singleton process"));
    }

    #[test]
    fn root_supervisor_rejects_boot_plan_timeout_outside_runtime_limits() {
        let mut bytecode = singleton_boot_bytecode(
            "Counter",
            RuntimeProcessKind::Agent,
            false,
            false,
            vec![Opcode::LoadConst(0), Opcode::Return],
            vec![Constant::Int(int(0))],
        );
        bytecode.runtime_boot_plan = RuntimeBootPlan::explicit_singleton("Counter");
        bytecode.runtime_boot_plan.singletons[0].init_timeout_ms = 0;
        let mut vm = VM::new(bytecode);

        let err = vm
            .ensure_root_supervisor_booted()
            .expect_err("invalid boot timeout should fail");

        assert!(err.message.contains("at least `1ms`"));
    }

    #[test]
    fn standby_singleton_decodes_ready_state_during_boot() {
        let bytecode = singleton_boot_bytecode(
            "Env",
            RuntimeProcessKind::Agent,
            true,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::LoadConst(2),
                Opcode::StructNew { field_count: 1 },
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![
                Constant::Tag(0),
                Constant::Tag(2),
                Constant::Str("ready".into()),
            ],
        );
        let mut vm = VM::new(bytecode);

        vm.ensure_root_supervisor_booted()
            .expect("boot should initialize lazy singleton");

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
            .expect("lazy singleton process should exist after boot");
        assert_eq!(instance.state_value, Some(Value::Str("ready".into())));
        assert!(!instance.standby_state_pending);
    }

    #[test]
    fn standby_singleton_pending_times_out_during_boot() {
        let mut bytecode = singleton_boot_bytecode(
            "Env",
            RuntimeProcessKind::Agent,
            true,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(1),
                Opcode::StructNew { field_count: 0 },
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![Constant::Tag(0), Constant::Tag(0)],
        );
        bytecode.runtime_boot_plan.singletons[0].init_timeout_ms = 1;
        bytecode
            .runtime_boot_plan
            .runtime_limits
            .pending_initial_retry_ms = 1;
        bytecode
            .runtime_boot_plan
            .runtime_limits
            .pending_max_retry_ms = 1;
        let mut vm = VM::new(bytecode);

        let err = vm
            .ensure_root_supervisor_booted()
            .expect_err("pending standby singleton should time out during VM init");

        assert_eq!(err.kind(), RuntimeErrorKind::ProcessInitTimeout);
        assert!(err.message.contains("Env"));
        assert!(err.context.details.contains(&"phase=VM::Init".into()));
        assert!(err.context.details.contains(&"process_name=Env".into()));
        assert!(err.context.details.contains(&"init_policy=Standby".into()));
        assert!(err
            .context
            .details
            .contains(&"trigger=standby_retry".into()));
        assert!(!vm.process_runtime.singleton_by_name.contains_key("Env"));
    }

    #[test]
    fn boot_failure_keeps_singleton_unpublished() {
        let mut bytecode = singleton_boot_bytecode(
            "Broken",
            RuntimeProcessKind::Agent,
            false,
            true,
            vec![
                Opcode::LoadConst(0),
                Opcode::LoadConst(2),
                Opcode::MakeError { template_id: 0 },
                Opcode::StructNew { field_count: 1 },
                Opcode::Return,
            ],
            vec![
                Constant::Tag(1),
                Constant::Str("BootFailure".into()),
                Constant::Str("bad boot".into()),
            ],
        );
        bytecode.error_templates = vec![test_source_error_template("BootFailure")];
        let mut vm = VM::new(bytecode);

        let err = vm
            .ensure_root_supervisor_booted()
            .expect_err("boot should fail");
        assert!(err.message.contains("Broken"));
        assert!(err.message.contains("bad boot"));
        assert!(err.context.details.contains(&"phase=VM::Init".into()));
        assert!(err.context.details.contains(&"process_name=Broken".into()));
        assert!(err.context.details.contains(&"init_policy=Eager".into()));
        assert!(err.context.details.contains(&"trigger=boot".into()));
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
            RuntimeProcessKind::Agent,
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
            Opcode::LoadConst(4),
            Opcode::MakeError { template_id: 0 },
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
                test_runtime_process_spec(
                    0,
                    "Good",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Singleton,
                    false,
                    0,
                    2,
                    None,
                ),
                test_runtime_process_spec(
                    0,
                    "Broken",
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Singleton,
                    false,
                    1,
                    3,
                    None,
                ),
            ],
        };
        bytecode.runtime_boot_plan.singletons = vec![
            SingletonBootEntry {
                process_name: "Good".into(),
                init_timeout_ms: 5_000,
                source: BootEntrySource::ExplicitConfig,
            },
            SingletonBootEntry {
                process_name: "Broken".into(),
                init_timeout_ms: 5_000,
                source: BootEntrySource::ExplicitConfig,
            },
        ];
        bytecode.error_templates = vec![test_source_error_template("BootFailure")];
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
    fn callable_future_wait_preserves_resume_context() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::LoadLocal(0), Opcode::Return]);
        bytecode.functions = vec![function_entry(0, 1, 1, 1, Some("Main::await_value"))];
        let mut vm = VM::new(bytecode);
        let future_id = vm.process_runtime.allocate_future(None, None, false);
        let callable = Callable {
            target: CallableTarget::Function(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata::default(),
        };

        let outcome = vm.drive_callable_until_wait(callable, vec![Value::PendingFuture(future_id)]);
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

        let resume =
            match vm.drive_callable_until_wait(callable, vec![Value::PendingFuture(future_id)]) {
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

        let outcome = vm.drive_callable_until_wait(
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

    pub(super) fn function_entry(
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            location_source: sindr::ir::ErrorLocationSource::SourceSpan,
            span_start: 0,
            span_end: 0,
            line: 1,
            column: 1,
            format: "{}".into(),
            num_params: 1,
            diagnostic: None,
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 1,
            error_templates: vec![ErrTemplate {
                id: 0,
                kind: "NewKind".into(),
                location_source: sindr::ir::ErrorLocationSource::SourceSpan,
                span_start: 10,
                span_end: 20,
                line: 2,
                column: 3,
                format: "{}".into(),
                num_params: 1,
                diagnostic: None,
            }],
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
    fn registered_sources_remain_immutable_and_survive_saved_snapshots() {
        let mut vm = VM::new(base_bytecode(vec![Opcode::Halt]));
        let original = sindr::ir::SourceFileEntry {
            source_id: 17,
            path: "REPL:1".into(),
            normalized_path: Some("REPL:1".into()),
            content_hash: None,
            text: Some("Err(NoneError)\n".into()),
        };
        vm.register_source(original.clone())
            .expect("new source must register");
        vm.register_source(original.clone())
            .expect("registering the same source again must be idempotent");

        let mut other_path = original.clone();
        other_path.path = "REPL:2".into();
        other_path.normalized_path = Some("REPL:2".into());
        let mut other_text = original.clone();
        other_text.text = Some("Ok(1)\n".into());
        for conflicting in [other_path, other_text] {
            let error = vm
                .register_source(conflicting)
                .expect_err("a source ID must retain its original path and text");
            assert!(error.message.contains("Source ID 17"), "{error}");
        }

        vm.set_source("later()\n".into(), "REPL".into());
        assert_eq!(vm.bytecode().sources, vec![original.clone()]);
        let snapshot = vm.snapshot_bytecode();
        assert_eq!(snapshot.sources, vec![original.clone()]);
        let bytes = snapshot.encode().expect("snapshot must encode");
        let restored = Bytecode::decode(&bytes).expect("snapshot must decode");
        assert_eq!(restored.sources, vec![original]);
    }

    #[test]
    fn make_error_resolves_embedded_source_by_exact_id_and_character_offset() {
        let base = 10 * sindr::ir::MODULE_SPAN_STRIDE as u32;
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::MakeError { template_id: 0 },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Str("failed".into())];
        bytecode.sources = vec![sindr::ir::SourceFileEntry {
            source_id: 9,
            path: "definition.srt".into(),
            normalized_path: None,
            content_hash: None,
            text: Some("前\n  1 =? 2\n".into()),
        }];
        bytecode.error_templates = vec![ErrTemplate {
            id: 0,
            kind: "Global::PatternMismatch".into(),
            location_source: sindr::ir::ErrorLocationSource::SourceSpan,
            span_start: base + 4,
            span_end: base + 5,
            line: 0,
            column: 0,
            format: String::new(),
            num_params: 1,
            diagnostic: None,
        }];
        let mut vm = VM::new(bytecode).with_source("main()\n".into(), "main.srt".into());
        vm.run().expect("registered source must execute");
        let Value::Error(error) = vm.last_value().unwrap() else {
            panic!("expected language Error");
        };
        assert_eq!(error.location.file, "definition.srt");
        assert_eq!((error.location.span_start, error.location.span_end), (4, 5));
        assert_eq!((error.location.line, error.location.column), (2, 3));
    }

    #[test]
    fn make_error_preserves_module_identity_without_source_metadata() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::MakeError { template_id: 0 },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Str("failed".into())];
        bytecode.error_templates = vec![ErrTemplate {
            id: 0,
            kind: "Failure".into(),
            location_source: sindr::ir::ErrorLocationSource::SourceSpan,
            span_start: sindr::ir::MODULE_SPAN_STRIDE as u32,
            span_end: sindr::ir::MODULE_SPAN_STRIDE as u32 + 1,
            line: 99,
            column: 42,
            format: String::new(),
            num_params: 1,
            diagnostic: None,
        }];
        for sources in [
            vec![],
            vec![sindr::ir::SourceFileEntry {
                source_id: 0,
                path: "definition.srt".into(),
                normalized_path: None,
                content_hash: None,
                text: None,
            }],
        ] {
            let has_entry = !sources.is_empty();
            bytecode.sources = sources;
            let mut vm =
                VM::new(bytecode.clone()).with_source("main()\n".into(), "main.srt".into());
            vm.run().expect("source text is optional metadata");
            let Value::Error(error) = vm.last_value().unwrap() else {
                panic!("expected language Error");
            };
            assert_eq!((error.location.line, error.location.column), (0, 0));
            if has_entry {
                assert_eq!(error.location.file, "definition.srt");
                assert_eq!((error.location.span_start, error.location.span_end), (0, 1));
            } else {
                assert_eq!(error.location.file, "<source:0>");
                assert_eq!(
                    error.location.span_start,
                    sindr::ir::MODULE_SPAN_STRIDE as u32
                );
                assert_eq!(
                    error.location.span_end,
                    sindr::ir::MODULE_SPAN_STRIDE as u32 + 1
                );
            }
        }
    }

    #[test]
    fn make_error_keeps_inline_pattern_source_span_inside_a_called_frame() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::MakeError { template_id: 0 },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Str("2".into())];
        bytecode.error_templates = vec![ErrTemplate {
            id: 0,
            kind: "Global::PatternMismatch".into(),
            location_source: sindr::ir::ErrorLocationSource::SourceSpan,
            span_start: 0,
            span_end: 1,
            line: 1,
            column: 1,
            format: String::new(),
            num_params: 1,
            diagnostic: Some(
                sindr::ir::RuntimeErrorDiagnosticTemplate::LiteralPatternMismatch {
                    lhs: "1".into(),
                },
            ),
        }];
        let mut vm = VM::new(bytecode).with_source("1 =? 2\nmain()\n".into(), "sample.srt".into());
        vm.frames[0].call_site = Some((7, 13));
        vm.run().expect("valid inline error should execute");
        let Value::Error(error) = vm.last_value().unwrap() else {
            panic!("expected pattern Error");
        };
        assert_eq!((error.location.span_start, error.location.span_end), (0, 1));
        assert_eq!((error.location.line, error.location.column), (1, 1));
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
                type_registry_base: 0,
                type_entries: Vec::new(),
                dbg_template_base: 0,
                dbg_templates: Vec::new(),
                callable_templates: Vec::new(),
                error_template_base: 0,
                error_templates: vec![ErrTemplate {
                    id: 0,
                    kind: "Boom".into(),
                    location_source: sindr::ir::ErrorLocationSource::ConstructorCallSite,
                    span_start: 0,
                    span_end: 5,
                    line: 1,
                    column: 1,
                    format: "{}".into(),
                    num_params: 1,
                    diagnostic: None,
                }],
                functions: Vec::new(),
                docs: Vec::new(),
                signatures: Vec::new(),
                runtime_process_specs: Vec::new(),
                runtime_boot_plan: Default::default(),
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
        let source = "Mod::safe_mod(10, 0)\n".to_string();
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
                    assert_eq!(rich.kind, "Global::ZeroDivisionError");
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
    fn uncons_match_result_error_keeps_builtin_call_location() {
        let source = r#"uncons("")"#.to_string();
        let span_end = source.trim_end().len() as u32;
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::CallBuiltin {
                builtin_id: sindr::builtin::builtin_id_by_name("uncons").unwrap(),
                arity: 1,
                span_start: 0,
                span_end,
            },
            Opcode::GetField { field_index: 1 },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Str(String::new())];
        bytecode.type_registry.register(TypeEntry {
            tag: 47,
            name: sindr::builtin::MATCH_RESULT_ERR_VARIANT
                .qualified_name
                .into(),
            kind: TypeKind::EnumVariant,
            field_names: vec!["discriminant".into(), "error".into()],
            private_flags: vec![false, false],
        });
        let mut vm = VM::new(bytecode).with_source(source, "extractor.srt".into());
        vm.run().unwrap();
        let Value::Error(error) = vm.last_value().unwrap() else {
            panic!("expected preserved Error")
        };
        assert_eq!(error.kind, "Global::PatternMismatch");
        assert_eq!(error.message, "Pattern did not match.");
        assert_eq!(error.location.file, "extractor.srt");
        assert_eq!((error.location.line, error.location.column), (1, 1));
        assert_eq!(
            (error.location.span_start, error.location.span_end),
            (0, span_end)
        );
        assert!(error.cause.is_none());
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: vec![function_entry(0, 2, 1, 0, Some("new"))],
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            type_registry_base: 0,
            type_entries: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
    fn type_registry_rejects_reserved_type_tag() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        let err = bytecode.type_registry.try_register(TypeEntry {
            tag: 0,
            name: "Bad".into(),
            kind: TypeKind::Struct,
            field_names: vec![],
            private_flags: vec![],
        });

        assert!(err
            .expect_err("reserved result tags must be rejected")
            .to_string()
            .contains("reserved result tag"));
    }

    #[test]
    fn type_registry_rejects_duplicate_type_tag() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode
            .type_registry
            .try_register(TypeEntry {
                tag: 10,
                name: "A".into(),
                kind: TypeKind::Struct,
                field_names: vec![],
                private_flags: vec![],
            })
            .expect("first type entry should be valid");
        let err = bytecode.type_registry.try_register(TypeEntry {
            tag: 10,
            name: "B".into(),
            kind: TypeKind::Record,
            field_names: vec![],
            private_flags: vec![],
        });

        assert!(err
            .expect_err("duplicate type tags must be rejected")
            .to_string()
            .contains("duplicate type tag"));
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
            type_registry_base: 0,
            type_entries: vec![TypeEntry {
                tag: 1,
                name: "Bad".into(),
                kind: TypeKind::Struct,
                field_names: vec![],
                private_flags: vec![],
            }],
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
            type_registry_base: 1,
            type_entries: vec![TypeEntry {
                tag: 10,
                name: "Duplicate".into(),
                kind: TypeKind::Record,
                field_names: vec![],
                private_flags: vec![],
            }],
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
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
    fn runtime_error_context_includes_stack_trace_for_direct_call() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Call {
                fun_idx: 0,
                arity: 0,
                span_start: 0,
                span_end: 7,
            },
            Opcode::Halt,
            Opcode::ListEmpty,
            Opcode::ListHead,
            Opcode::Return,
        ]);
        bytecode.functions = vec![function_entry(0, 2, 0, 0, Some("Main::inner"))];
        let err = VM::new(bytecode)
            .with_source("inner()\n".into(), "sample.srt".into())
            .run()
            .expect_err("must fail");

        assert!(err.message.contains("ListHead on empty list"));
        assert_eq!(err.context.stack_trace.len(), 1);
        let frame = &err.context.stack_trace[0];
        assert_eq!(frame.function.as_deref(), Some("inner"));
        assert_eq!(
            frame.location.as_ref().map(|location| {
                (
                    location.file.as_str(),
                    location.line,
                    location.column,
                    location.span_start,
                    location.span_end,
                )
            }),
            Some(("sample.srt", 1, 1, 0, 7))
        );
    }

    #[test]
    fn builtin_runtime_error_context_includes_builtin_stack_trace() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::CallBuiltin {
                builtin_id: builtin_id("len"),
                arity: 1,
                span_start: 0,
                span_end: 7,
            },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(1))];
        let err = VM::new(bytecode)
            .with_source("len([])\n".into(), "sample.srt".into())
            .run()
            .expect_err("must fail");

        assert!(err.message.contains("len expects List"));
        assert_eq!(err.context.stack_trace.len(), 1);
        let frame = &err.context.stack_trace[0];
        assert_eq!(frame.function.as_deref(), Some("len"));
        assert_eq!(frame.call_kind, RuntimeCallKind::Builtin);
        assert_eq!(
            frame.location.as_ref().map(|location| {
                (
                    location.file.as_str(),
                    location.line,
                    location.column,
                    location.span_start,
                    location.span_end,
                )
            }),
            Some(("sample.srt", 1, 1, 0, 7))
        );
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
    fn shift_and_bit_index_opcodes_execute_successfully() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::ShlInt,
            Opcode::LoadConst(2),
            Opcode::LoadConst(3),
            Opcode::ShrInt,
            Opcode::LoadConst(4),
            Opcode::LoadConst(5),
            Opcode::TestBitInt,
            Opcode::LoadConst(6),
            Opcode::LoadConst(3),
            Opcode::SetBitInt,
            Opcode::LoadConst(7),
            Opcode::LoadConst(3),
            Opcode::ClearBitInt,
            Opcode::LoadConst(4),
            Opcode::LoadConst(5),
            Opcode::ToggleBitInt,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![
            Constant::Int(int(1)),
            Constant::Int(int(3)),
            Constant::Int(int(8)),
            Constant::Int(int(1)),
            Constant::Int(int(5)),
            Constant::Int(int(0)),
            Constant::Int(int(0)),
            Constant::Int(int(7)),
        ];

        let mut vm = VM::new(bytecode);
        vm.run().expect("run should succeed");

        assert_eq!(
            vm.stack,
            vec![
                Value::Tagged {
                    tag: 0,
                    fields: vec![Value::Int(int(8))],
                },
                Value::Tagged {
                    tag: 0,
                    fields: vec![Value::Int(int(4))],
                },
                Value::Tagged {
                    tag: 0,
                    fields: vec![Value::Bool(true)],
                },
                Value::Tagged {
                    tag: 0,
                    fields: vec![Value::Int(int(2))],
                },
                Value::Tagged {
                    tag: 0,
                    fields: vec![Value::Int(int(5))],
                },
                Value::Tagged {
                    tag: 0,
                    fields: vec![Value::Int(int(4))],
                },
            ]
        );
    }

    #[test]
    fn shift_and_bit_index_opcodes_preserve_negative_index_errors() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::ShlInt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::ShrInt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::TestBitInt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::SetBitInt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::ClearBitInt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::ToggleBitInt,
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(1)), Constant::Int(int(-1))];

        let mut vm = VM::new(bytecode);
        vm.run().expect("run should succeed");

        let expected_kinds = [
            "Global::NegativeShiftCount",
            "Global::NegativeShiftCount",
            "Global::NegativeBitIndex",
            "Global::NegativeBitIndex",
            "Global::NegativeBitIndex",
            "Global::NegativeBitIndex",
        ];
        for (value, expected_kind) in vm.stack.iter().zip(expected_kinds) {
            match value {
                Value::Tagged { tag: 1, fields } => match fields.first() {
                    Some(Value::Error(rich)) => assert_eq!(rich.kind, expected_kind),
                    other => panic!("expected Err(Error), got {other:?}"),
                },
                other => panic!("expected Err result, got {other:?}"),
            }
        }
    }

    #[test]
    fn match_result_access_rejects_malformed_carriers() {
        let malformed = [
            ("MatchResult::Ok", vec![Value::Int(int(0))]),
            (
                "MatchResult::Ok",
                vec![Value::Int(int(0)), Value::Unit, Value::Unit],
            ),
            ("MatchResult::Ok", vec![Value::Int(int(1)), Value::Unit]),
            ("MatchResult::Ok", vec![Value::Unit, Value::Unit]),
            ("Global::MatchResult::Ok", vec![Value::Unit, Value::Unit]),
            ("MatchResult::Err", vec![Value::Int(int(1)), Value::Unit]),
        ];
        for (name, fields) in malformed {
            for opcode in [
                Opcode::GetTag,
                Opcode::GetField { field_index: 1 },
                Opcode::EqLocalTag {
                    local_idx: 0,
                    tag_const_idx: 0,
                },
                Opcode::JumpIfLocalTagEq {
                    local_idx: 0,
                    tag_const_idx: 0,
                    target_pc: 1,
                },
                Opcode::JumpIfLocalTagNe {
                    local_idx: 0,
                    tag_const_idx: 0,
                    target_pc: 1,
                },
            ] {
                let mut bytecode = base_bytecode(vec![opcode, Opcode::Halt]);
                bytecode.constants = vec![Constant::Tag(47)];
                bytecode.type_registry.register(TypeEntry {
                    tag: 47,
                    name: name.into(),
                    kind: TypeKind::EnumVariant,
                    field_names: vec!["discriminant".into(), "payload".into()],
                    private_flags: vec![false, false],
                });
                let mut vm = VM::new(bytecode);
                let mut context = top_level_context(0, 1);
                let carrier = Value::Tagged {
                    tag: 47,
                    fields: fields.clone(),
                };
                context.stack.push(carrier.clone());
                context.frames[0].locals[0] = carrier;
                let error = match vm.step_context(&mut context) {
                    StepOutcome::RuntimeError(error) => error,
                    other => panic!("malformed carrier must fail closed, got {other:?}"),
                };
                assert!(
                    error.message.contains("Invalid MatchResult representation"),
                    "{}",
                    error.message
                );
            }
        }
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
            Opcode::MakeOk,
            Opcode::JumpIfLocalTagEq {
                local_idx: 0,
                tag_const_idx: 1,
                target_pc: 3,
            },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(6)), Constant::Tag(0)];

        let mut vm = VM::new(bytecode);
        vm.enable_observation(VmObservationOptions::default());
        let mut ctx = top_level_context(0, 1);

        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected load const to continue, got {other:?}"),
        }
        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected make ok to continue, got {other:?}"),
        }
        ctx.frames[0].locals[0] = ctx
            .stack
            .last()
            .cloned()
            .expect("make ok should leave a tagged result on the stack");
        match vm.step_context(&mut ctx) {
            StepOutcome::Continue => {}
            other => panic!("expected fused local tag jump to continue, got {other:?}"),
        }
        match vm.step_context(&mut ctx) {
            StepOutcome::Halt(value) => assert_eq!(
                value,
                Value::Tagged {
                    tag: 0,
                    fields: vec![Value::Int(int(6))],
                }
            ),
            other => panic!("expected halt, got {other:?}"),
        }

        let observation = vm.observation().expect("observation should exist");
        assert_eq!(observation.stats.executed_opcodes, 4);
        assert_eq!(observation.stats.per_opcode.get("LoadConst"), Some(&1));
        assert_eq!(observation.stats.per_opcode.get("MakeOk"), Some(&1));
        assert_eq!(
            observation.stats.per_opcode.get("JumpIfLocalTagEq"),
            Some(&1)
        );
        assert_eq!(observation.stats.per_opcode.get("Halt"), Some(&1));
        assert_eq!(observation.stats.max_stack_depth, 1);
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
    fn tail_call_closure_returns_user_function_result_to_caller() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadFunctionRef(1),
            Opcode::LoadConst(0),
            Opcode::Call {
                fun_idx: 0,
                arity: 2,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::LoadLocal(1),
            Opcode::TailCallClosure {
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::LoadLocal(0),
            Opcode::LoadConst(1),
            Opcode::AddInt,
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Int(int(41)), Constant::Int(int(1))];
        bytecode.functions = vec![
            function_entry(0, 4, 2, 2, Some("Main::apply_tail")),
            function_entry(1, 7, 1, 1, Some("Main::add1")),
        ];

        let mut vm = VM::new(bytecode);
        vm.run().expect("run should succeed");

        assert_eq!(vm.last_result, Some(Value::Int(int(42))));
    }

    #[test]
    fn tail_call_closure_preserves_lexical_capture_order() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadFunctionRef(1),
            Opcode::LoadConst(0),
            Opcode::CaptureClosure(1),
            Opcode::LoadConst(1),
            Opcode::Call {
                fun_idx: 0,
                arity: 2,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::LoadLocal(1),
            Opcode::TailCallClosure {
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::LoadLocal(0),
            Opcode::LoadLocal(1),
            Opcode::AddInt,
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Int(int(10)), Constant::Int(int(32))];
        bytecode.functions = vec![
            function_entry(0, 6, 2, 2, Some("Main::apply_tail")),
            function_entry(1, 9, 2, 2, Some("Main::add_base")),
        ];

        let mut vm = VM::new(bytecode);
        vm.run().expect("run should succeed");

        assert_eq!(vm.last_result, Some(Value::Int(int(42))));
    }

    #[test]
    fn tail_call_closure_returns_builtin_result_to_caller() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadBuiltinRef(builtin_id("to_string")),
            Opcode::LoadConst(0),
            Opcode::Call {
                fun_idx: 0,
                arity: 2,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::LoadLocal(1),
            Opcode::TailCallClosure {
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
        ]);
        bytecode.constants = vec![Constant::Int(int(42))];
        bytecode.functions = vec![function_entry(0, 4, 2, 2, Some("Main::apply_tail"))];

        let mut vm = VM::new(bytecode);
        vm.run().expect("run should succeed");

        assert_eq!(vm.last_result, Some(Value::Str("42".into())));
    }

    #[test]
    fn tail_call_closure_template_target_is_not_user_function_tco() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadCallableTemplateRef(0),
            Opcode::LoadConst(0),
            Opcode::Call {
                fun_idx: 0,
                arity: 2,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::LoadLocal(1),
            Opcode::TailCallClosure {
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::LoadLocal(0),
            Opcode::LoadConst(1),
            Opcode::AddInt,
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Int(int(41)), Constant::Int(int(1))];
        bytecode.functions = vec![
            function_entry(0, 4, 2, 2, Some("Main::apply_tail")),
            function_entry(1, 7, 1, 1, Some("Main::add1")),
        ];
        bytecode.callable_templates = vec![CallableTemplate {
            template_id: 0,
            kind: CallableTemplateKind::PartialDirectCall {
                target: CallableTemplateDirectTarget::Function(1),
                arg_sources: vec![CallableTemplateArg::Runtime(0)],
            },
            metadata: Default::default(),
        }];

        let mut vm = VM::new(bytecode);
        vm.enable_observation(VmObservationOptions::default());
        vm.run().expect("run should succeed");

        let observation = vm.observation().expect("observation should exist");
        assert_eq!(vm.last_result, Some(Value::Int(int(42))));
        assert_eq!(observation.stats.tail_calls_optimized, 0);
    }

    #[test]
    fn tail_call_closure_rejects_non_callable_target() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::Call {
                fun_idx: 0,
                arity: 2,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::LoadLocal(1),
            Opcode::TailCallClosure {
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
        ]);
        bytecode.constants = vec![Constant::Int(int(0)), Constant::Int(int(42))];
        bytecode.functions = vec![function_entry(0, 4, 2, 2, Some("Main::bad_apply"))];

        let err = VM::new(bytecode).run().expect_err("must fail");

        assert!(err.message.contains("CallClosure expects a callable value"));
    }

    #[test]
    fn call_closure_executes_partial_direct_call_template() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::CallClosure {
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::LoadLocal(1),
            Opcode::AddInt,
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Int(int(41)), Constant::Int(int(1))];
        bytecode.functions = vec![function_entry(0, 3, 2, 2, Some("Main::add1"))];
        bytecode.callable_templates = vec![CallableTemplate {
            template_id: 0,
            kind: CallableTemplateKind::PartialDirectCall {
                target: CallableTemplateDirectTarget::Function(0),
                arg_sources: vec![
                    CallableTemplateArg::Runtime(0),
                    CallableTemplateArg::Bound(0),
                ],
            },
            metadata: Default::default(),
        }];

        let mut vm = VM::new(bytecode);
        vm.stack.push(Value::Callable(Callable {
            target: CallableTarget::Template(0),
            lexical_captures: vec![Value::Int(int(1))],
            metadata: CallableMetadata::default(),
        }));
        vm.run().expect("run should succeed");

        assert_eq!(vm.last_result, Some(Value::Int(int(42))));
    }

    #[test]
    fn call_closure_executes_inject_direct_call_template() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::CallClosure {
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
        ]);
        bytecode.constants = vec![Constant::Int(int(41))];
        bytecode.callable_templates = vec![CallableTemplate {
            template_id: 0,
            kind: CallableTemplateKind::InjectDirectCall {
                target: CallableTemplateDirectTarget::Builtin(builtin_id("to_string")),
                bound_arg_count: 0,
            },
            metadata: Default::default(),
        }];

        let mut vm = VM::new(bytecode);
        vm.stack.push(Value::Callable(Callable {
            target: CallableTarget::Template(0),
            lexical_captures: Vec::new(),
            metadata: CallableMetadata::default(),
        }));
        vm.run().expect("run should succeed");

        assert_eq!(vm.last_result, Some(Value::Str("41".into())));
    }

    #[test]
    fn call_closure_executes_compose_direct_template() {
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadConst(0),
            Opcode::CallClosure {
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::LoadConst(1),
            Opcode::AddInt,
            Opcode::Return,
            Opcode::LoadLocal(0),
            Opcode::LoadConst(2),
            Opcode::MulInt,
            Opcode::Return,
        ]);
        bytecode.constants = vec![
            Constant::Int(int(20)),
            Constant::Int(int(1)),
            Constant::Int(int(2)),
        ];
        bytecode.functions = vec![
            function_entry(0, 3, 1, 1, Some("Main::inc")),
            function_entry(1, 7, 1, 1, Some("Main::double")),
        ];
        bytecode.callable_templates = vec![CallableTemplate {
            template_id: 0,
            kind: CallableTemplateKind::ComposeDirect {
                flavor: CallableTemplateComposeFlavor::Plain,
            },
            metadata: Default::default(),
        }];

        let mut vm = VM::new(bytecode);
        vm.stack.push(Value::Callable(Callable {
            target: CallableTarget::Template(0),
            lexical_captures: vec![
                Value::Callable(Callable {
                    target: CallableTarget::Function(0),
                    lexical_captures: Vec::new(),
                    metadata: CallableMetadata::default(),
                }),
                Value::Callable(Callable {
                    target: CallableTarget::Function(1),
                    lexical_captures: Vec::new(),
                    metadata: CallableMetadata::default(),
                }),
            ],
            metadata: CallableMetadata::default(),
        }));
        vm.run().expect("run should succeed");

        assert_eq!(vm.last_result, Some(Value::Int(int(42))));
    }

    #[test]
    fn compose_callback_shares_quantum_and_resumes_once() {
        let mut bytecode = base_bytecode(vec![
            Opcode::CallClosure {
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::LoadConst(0),
            Opcode::AddInt,
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Int(int(1))];
        bytecode.functions = vec![function_entry(0, 2, 1, 1, Some("Main::inc"))];
        bytecode.callable_templates = vec![CallableTemplate {
            template_id: 0,
            kind: CallableTemplateKind::ComposeDirect {
                flavor: CallableTemplateComposeFlavor::Plain,
            },
            metadata: Default::default(),
        }];
        let mut vm = VM::new(bytecode);
        let inc = Value::Callable(vm.callable_for_function(0));
        let mut ctx = top_level_context(0, 0);
        ctx.stack = vec![
            Value::Callable(Callable {
                target: CallableTarget::Template(0),
                lexical_captures: vec![inc.clone(), inc],
                metadata: Default::default(),
            }),
            Value::Int(int(40)),
        ];
        let original = ctx.stack.clone();
        assert!(matches!(
            vm.run_quantum(&mut ctx, &mut Budget::new(0)),
            ProcessRunOutcome::QuantumExpired
        ));
        assert_eq!(ctx.stack, original);
        assert!(matches!(
            vm.run_quantum(&mut ctx, &mut Budget::new(1)),
            ProcessRunOutcome::QuantumExpired
        ));
        assert_ne!(
            ctx.stack.last(),
            Some(&Value::Int(int(42))),
            "callback must not run to completion inside dispatch"
        );
        let mut steps = 0;
        loop {
            steps += 1;
            match vm.run_quantum(&mut ctx, &mut Budget::new(1)) {
                ProcessRunOutcome::QuantumExpired => assert!(steps < 30),
                ProcessRunOutcome::Halted(value) => {
                    assert_eq!(value, Value::Int(int(42)));
                    break;
                }
                other => panic!("unexpected outcome {other:?}"),
            }
        }
        assert!(steps >= 8);
    }

    #[test]
    fn file_callback_keeps_resource_across_quantum_and_wait_then_closes_on_success_or_failure() {
        for fail in [false, true] {
            let dir = sandbox_dir(if fail {
                "file-continuation-fail"
            } else {
                "file-continuation-ok"
            });
            let path = dir.join("output.txt");
            let mut code = vec![Opcode::LoadLocal(0), Opcode::Pop];
            if fail {
                code.extend([Opcode::LoadConst(0), Opcode::ListHead]);
            } else {
                code.push(Opcode::LoadLocal(1));
            }
            code.push(Opcode::Return);
            let mut bytecode = base_bytecode(code);
            bytecode.constants = vec![Constant::Int(int(7))];
            bytecode.functions = vec![function_entry(0, 0, 2, 2, Some("Main::body"))];
            bytecode.type_registry.register(TypeEntry {
                tag: 10,
                name: "Write".into(),
                kind: TypeKind::EnumVariant,
                field_names: Vec::new(),
                private_flags: Vec::new(),
            });
            let mut vm = VM::new(bytecode).with_source(
                "File::with_open(path, mode, body)".into(),
                "wrapper.srt".into(),
            );
            vm.frames[0].call_site = Some((0, 4));
            vm.frames[0].trace_frame = Some(vm.trace_frame_for_builtin("file_with_open", 0, 4));
            let future = vm.process_runtime.allocate_future(None, None, false);
            let mut body = vm.callable_for_function(0);
            body.lexical_captures.push(Value::PendingFuture(future));
            let callable = Callable {
                target: CallableTarget::Builtin(builtin_id("file_with_open")),
                lexical_captures: Vec::new(),
                metadata: Default::default(),
            };
            let mut context = vm
                .prepare_callable_context(
                    callable,
                    vec![
                        Value::Str(path.to_string_lossy().into()),
                        Value::Tagged {
                            tag: 10,
                            fields: Vec::new(),
                        },
                        Value::Callable(body),
                    ],
                )
                .unwrap();
            loop {
                match vm.run_quantum(&mut context, &mut Budget::new(1)) {
                    ProcessRunOutcome::QuantumExpired => {}
                    ProcessRunOutcome::Pending(id) => {
                        assert_eq!(id, future);
                        break;
                    }
                    other => panic!("expected suspended callback, got {other:?}"),
                }
            }
            assert_eq!(vm.open_file_count(), 1);
            let handle = *vm.open_files.keys().next().unwrap();
            vm.write_file_chunk(handle, "kept alive").unwrap();
            vm.process_runtime.resolve_future(future, Value::Unit);
            loop {
                match vm.run_quantum(&mut context, &mut Budget::new(1)) {
                    ProcessRunOutcome::QuantumExpired => {}
                    ProcessRunOutcome::Halted(Value::FileHandle(result)) if !fail => {
                        assert_eq!(result.id, handle);
                        break;
                    }
                    ProcessRunOutcome::Failed(error) if fail => {
                        assert!(error.message.contains("List"));
                        assert!(error
                            .context
                            .stack_trace
                            .iter()
                            .any(|frame| frame.function.as_deref() == Some("file_with_open")));
                        assert!(error.context.call_site.is_some());
                        break;
                    }
                    other => panic!("unexpected callback completion {other:?}"),
                }
            }
            assert_eq!(vm.open_file_count(), 0);
            assert_eq!(fs::read_to_string(&path).unwrap(), "kept alive");
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn cancelling_waiting_file_callback_unwinds_without_running_callback_again() {
        let dir = sandbox_dir("file-continuation-cancel");
        let path = dir.join("output.txt");
        let mut bytecode = base_bytecode(vec![
            Opcode::LoadLocal(0),
            Opcode::CallBuiltin {
                builtin_id: builtin_id("print"),
                arity: 1,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Return,
        ]);
        bytecode.functions = vec![function_entry(0, 0, 2, 2, Some("Main::body"))];
        let mut vm = VM::new(bytecode).with_output_capture();
        let handle = vm
            .open_file_resource(&path.to_string_lossy(), VmFileMode::Write)
            .unwrap();
        let future = vm.process_runtime.allocate_future(None, None, false);
        let mut body = vm.callable_for_function(0);
        body.lexical_captures.push(Value::PendingFuture(future));
        let outcome = crate::builtin::BuiltinOutcome::Call {
            callable: body,
            args: vec![Value::FileHandle(handle.clone())],
            continuation: crate::builtin::BuiltinContinuation::FileWithOpen {
                path: path.to_string_lossy().into(),
                handle,
            },
        };
        let mut context = vm.prepare_builtin_context(outcome).unwrap();
        assert!(
            matches!(vm.run_quantum(&mut context,&mut Budget::new(32)),ProcessRunOutcome::Pending(id) if id==future)
        );
        VM::cancel_execution_context(&mut context, crate::error::RuntimeError::new("cancelled"));
        assert!(
            matches!(vm.run_quantum(&mut context,&mut Budget::new(32)),ProcessRunOutcome::Failed(error) if error.message=="cancelled")
        );
        assert_eq!(vm.open_file_count(), 0);
        assert!(vm.output.unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancelling_file_dispatch_at_every_quantum_boundary_closes_owned_handle() {
        let dir = sandbox_dir("file-start-cancellation");
        for stop_after in 0..24 {
            let mut bytecode = base_bytecode(vec![Opcode::LoadLocal(0), Opcode::Return]);
            bytecode.functions = vec![function_entry(0, 0, 2, 2, Some("Main::body"))];
            bytecode.type_registry.register(TypeEntry {
                tag: 10,
                name: "Write".into(),
                kind: TypeKind::EnumVariant,
                field_names: Vec::new(),
                private_flags: Vec::new(),
            });
            let mut vm = VM::new(bytecode);
            let future = vm.process_runtime.allocate_future(None, None, false);
            let mut body = vm.callable_for_function(0);
            body.lexical_captures.push(Value::PendingFuture(future));
            let callable = Callable {
                target: CallableTarget::Builtin(builtin_id("file_with_open")),
                lexical_captures: Vec::new(),
                metadata: Default::default(),
            };
            let mut context = vm
                .prepare_callable_context(
                    callable,
                    vec![
                        Value::Str(dir.join("out.txt").to_string_lossy().into()),
                        Value::Tagged {
                            tag: 10,
                            fields: Vec::new(),
                        },
                        Value::Callable(body),
                    ],
                )
                .unwrap();
            for _ in 0..(stop_after % 12) {
                match vm.run_quantum(&mut context, &mut Budget::new(1)) {
                    ProcessRunOutcome::QuantumExpired => {}
                    ProcessRunOutcome::Pending(_) => break,
                    other => panic!("unexpected pre-cancellation outcome {other:?}"),
                }
            }
            if stop_after >= 12 {
                if let Some(handle) = vm.open_files.keys().next().copied() {
                    vm.close_file_resource(handle).unwrap();
                }
            }
            VM::cancel_execution_context(
                &mut context,
                crate::error::RuntimeError::new("cancelled"),
            );
            assert!(
                matches!(vm.run_quantum(&mut context,&mut Budget::new(32)),ProcessRunOutcome::Failed(error) if error.message=="cancelled")
            );
            assert_eq!(
                vm.open_file_count(),
                0,
                "resource leaked at boundary {stop_after}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn batch_halt_keeps_background_file_callback_open_until_drained() {
        let dir = sandbox_dir("batch-background-file");
        let mut bytecode = base_bytecode(vec![Opcode::Halt, Opcode::LoadLocal(0), Opcode::Return]);
        bytecode.functions = vec![function_entry(0, 1, 2, 2, Some("Main::body"))];
        bytecode.type_registry.register(TypeEntry {
            tag: 10,
            name: "Write".into(),
            kind: TypeKind::EnumVariant,
            field_names: Vec::new(),
            private_flags: Vec::new(),
        });
        let mut vm = VM::new(bytecode);
        let future = vm.process_runtime.allocate_future(None, None, false);
        let mut body = vm.callable_for_function(0);
        body.lexical_captures.push(Value::PendingFuture(future));
        let launch = Callable {
            target: CallableTarget::Builtin(builtin_id("file_with_open")),
            lexical_captures: vec![
                Value::Str(dir.join("out.txt").to_string_lossy().into()),
                Value::Tagged {
                    tag: 10,
                    fields: Vec::new(),
                },
                Value::Callable(body),
            ],
            metadata: Default::default(),
        };
        vm.invoke_task(launch, TaskMode::Launch).unwrap();
        assert_eq!(vm.open_file_count(), 1);
        vm.run().unwrap();
        assert_eq!(
            vm.open_file_count(),
            1,
            "root Halt must retain the suspended callback's file"
        );
        vm.process_runtime.resolve_future(future, Value::Unit);
        vm.drain_background_tasks().unwrap();
        assert_eq!(vm.open_file_count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn checkpoint_restores_suspended_file_callback_after_live_callback_closed_it() {
        let dir = sandbox_dir("checkpoint-background-file");
        let path = dir.join("out.txt");
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadLocal(0),
            Opcode::Pop,
            Opcode::LoadLocal(1),
            Opcode::LoadConst(0),
            Opcode::CallBuiltin {
                builtin_id: builtin_id("file_write_chunk"),
                arity: 2,
                span_start: 0,
                span_end: 0,
            },
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Str("after".into())];
        bytecode.functions = vec![function_entry(0, 1, 2, 2, Some("Main::body"))];
        bytecode.type_registry.register(TypeEntry {
            tag: 10,
            name: "Write".into(),
            kind: TypeKind::EnumVariant,
            field_names: Vec::new(),
            private_flags: Vec::new(),
        });
        let mut vm = VM::new(bytecode);
        let future = vm.process_runtime.allocate_future(None, None, false);
        let mut body = vm.callable_for_function(0);
        body.lexical_captures.push(Value::PendingFuture(future));
        let launch = Callable {
            target: CallableTarget::Builtin(builtin_id("file_with_open")),
            lexical_captures: vec![
                Value::Str(path.to_string_lossy().into()),
                Value::Tagged {
                    tag: 10,
                    fields: Vec::new(),
                },
                Value::Callable(body),
            ],
            metadata: Default::default(),
        };
        vm.invoke_task(launch, TaskMode::Launch).unwrap();
        let handle = *vm.open_files.keys().next().unwrap();
        vm.write_file_chunk(handle, "before-").unwrap();
        let checkpoint = vm.checkpoint_for_chunk(&BytecodeChunk {
            opcodes: Vec::new(),
            source_map: None,
            const_base: 0,
            constants: Vec::new(),
            new_locals: 0,
            type_registry_base: 0,
            type_entries: Vec::new(),
            error_template_base: 0,
            error_templates: Vec::new(),
            dbg_template_base: 0,
            dbg_templates: Vec::new(),
            callable_templates: Vec::new(),
            functions: Vec::new(),
            docs: Vec::new(),
            signatures: Vec::new(),
            runtime_process_specs: Vec::new(),
            runtime_boot_plan: Default::default(),
        });
        vm.process_runtime.resolve_future(future, Value::Unit);
        vm.drain_background_tasks().unwrap();
        assert_eq!(vm.open_file_count(), 0);
        assert_eq!(fs::read_to_string(&path).unwrap(), "before-after");
        vm.rollback_to_checkpoint(checkpoint);
        assert_eq!(vm.open_file_count(), 1);
        assert!(vm.ready_future_value(future).is_none());
        vm.write_file_chunk(handle, "resumed-").unwrap();
        vm.process_runtime.resolve_future(future, Value::Unit);
        vm.drain_background_tasks().unwrap();
        assert_eq!(vm.open_file_count(), 0);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "before-afterresumed-after"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unresolved_wait_unwinds_file_callback_before_host_error() {
        let dir = sandbox_dir("file-unresolved-wait");
        let path = dir.join("out.txt");
        let mut bytecode = base_bytecode(vec![Opcode::LoadLocal(0), Opcode::Return]);
        bytecode.functions = vec![function_entry(0, 0, 2, 2, Some("Main::body"))];
        let mut vm = VM::new(bytecode);
        let handle = vm
            .open_file_resource(&path.to_string_lossy(), VmFileMode::Write)
            .unwrap();
        let future = vm.process_runtime.allocate_future(None, None, false);
        let mut body = vm.callable_for_function(0);
        body.lexical_captures.push(Value::PendingFuture(future));
        let outcome = crate::builtin::BuiltinOutcome::Call {
            callable: body,
            args: vec![Value::FileHandle(handle.clone())],
            continuation: crate::builtin::BuiltinContinuation::FileWithOpen {
                path: path.to_string_lossy().into(),
                handle,
            },
        };
        let error = vm.drive_builtin_outcome(outcome).unwrap_err();
        assert!(error.message.contains("unresolved future"));
        assert_eq!(vm.open_file_count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recover_callback_wait_resumes_once_and_validates_result_with_source_trace() {
        for valid in [true, false] {
            let mut bytecode = base_bytecode(vec![
                Opcode::LoadConst(0),
                Opcode::CallBuiltin {
                    builtin_id: builtin_id("print"),
                    arity: 1,
                    span_start: 0,
                    span_end: 1,
                },
                Opcode::Pop,
                Opcode::LoadLocal(0),
                Opcode::Return,
            ]);
            bytecode.constants = vec![Constant::Str("called".into())];
            bytecode.functions = vec![function_entry(0, 0, 2, 2, Some("Main::handler"))];
            let mut vm = VM::new(bytecode)
                .with_output_capture()
                .with_source("recover".into(), "recover.srt".into());
            vm.frames[0].call_site = Some((0, 7));
            vm.frames[0].trace_frame = Some(vm.trace_frame_for_builtin("__recover_kind", 0, 7));
            let future = vm.process_runtime.allocate_future(None, None, false);
            let mut handler = vm.callable_for_function(0);
            handler.lexical_captures.push(Value::PendingFuture(future));
            let error = vm.process_error("Retry", "retry");
            let kind = error.kind.clone();
            let callable = Callable {
                target: CallableTarget::Builtin(builtin_id("__recover_kind")),
                lexical_captures: Vec::new(),
                metadata: Default::default(),
            };
            let mut context = vm
                .prepare_callable_context(
                    callable,
                    vec![
                        super::err_vm_result(error),
                        Value::Str(kind.into()),
                        Value::Callable(handler),
                    ],
                )
                .unwrap();
            assert!(
                matches!(vm.run_quantum(&mut context,&mut Budget::new(64)),ProcessRunOutcome::Pending(id) if id==future)
            );
            assert_eq!(vm.output.as_ref().unwrap(), &["called".to_string()]);
            vm.process_runtime.resolve_future(
                future,
                if valid {
                    ok_vm_result(Value::Int(int(42)))
                } else {
                    Value::Int(int(42))
                },
            );
            let result = vm.run_quantum(&mut context, &mut Budget::new(64));
            if valid {
                assert!(
                    matches!(result,ProcessRunOutcome::Halted(value) if value==ok_vm_result(Value::Int(int(42))))
                );
            } else {
                match result {
                    ProcessRunOutcome::Failed(error) => {
                        assert!(error.message.contains("handler result"));
                        assert!(error.context.call_site.is_some());
                        assert!(error
                            .context
                            .stack_trace
                            .iter()
                            .any(|f| f.function.as_deref() == Some("__recover_kind")));
                    }
                    other => panic!("expected invalid handler result error, got {other:?}"),
                }
            }
            assert_eq!(vm.output.as_ref().unwrap(), &["called".to_string()]);
            assert!(context.continuations.is_empty());
        }
    }

    #[test]
    fn ordinary_driver_propagates_runnable_process_failure() {
        let mut opcodes = Vec::new();
        for _ in 0..70 {
            opcodes.extend([Opcode::LoadConst(0), Opcode::Pop]);
        }
        opcodes.push(Opcode::Halt);
        let failed_pc = opcodes.len();
        opcodes.extend([Opcode::LoadConst(0), Opcode::ListHead]);
        let mut bytecode = test_process_bytecode("Worker", opcodes);
        bytecode.constants = vec![Constant::Int(int(7))];
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_process_instance("Worker".into(), Some(Value::Unit), None, None)
            .unwrap();
        vm.process_runtime
            .processes
            .get_mut(&pid)
            .unwrap()
            .execution_context = Some(top_level_context(failed_pc, 0));
        vm.process_runtime.enqueue_runnable(pid);
        match vm.run_until_outcome(0, ExecutionTarget::TopLevel) {
            StepOutcome::RuntimeError(error) => assert!(error.message.contains("List")),
            other => panic!("expected process failure from common driver, got {other:?}"),
        }
        assert_eq!(
            vm.process_runtime.processes[&pid].status,
            ProcessStatus::Failed
        );
    }

    #[test]
    fn observation_includes_process_runtime_counters() {
        let bytecode = singleton_boot_bytecode(
            "Counter",
            RuntimeProcessKind::Agent,
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
            RuntimeProcessKind::Agent,
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
        assert_eq!(snapshot.specs[0].type_name, "Counter");
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
    fn finalize_worker_stop_cleans_reply_waiters_and_supervisor_membership() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                "Worker",
                RuntimeProcessKind::GenServer,
                RuntimeProcessInstance::Worker,
                false,
                0,
                0,
                None,
            )],
        };
        let mut vm = VM::new(bytecode);
        let pid = vm
            .allocate_supervised_worker(
                "Worker".into(),
                Some(Value::Int(int(41))),
                "DynamicSupervisor".into(),
            )
            .expect("worker allocation should succeed");
        let future_id = vm.process_runtime.allocate_future(Some(pid), Some(3), true);
        let correlation_id = vm.process_runtime.allocate_correlation_id();
        vm.process_runtime
            .register_reply_waiter(correlation_id, future_id);
        vm.process_runtime
            .mark_process_waiting(pid, super::ProcessWaitReason::Reply(correlation_id));

        let resumed =
            vm.finalize_process_stop(pid, Some(super::ok_vm_result(Value::Int(int(99)))), false);

        assert!(resumed.is_empty());
        assert!(matches!(
            vm.process_runtime
                .processes
                .get(&pid)
                .expect("process exists")
                .status,
            super::ProcessStatus::Stopped
        ));
        assert!(vm.process_runtime.reply_table.is_empty());
        assert!(!vm.process_runtime.waiting_table.contains_key(&pid));
        assert!(vm.process_runtime.deadline_queue.is_empty());
        assert!(vm
            .process_runtime
            .root_supervisor
            .child_table
            .get("DynamicSupervisor")
            .is_none_or(|children| !children.contains(&pid)));
        assert!(matches!(
            vm.process_runtime
                .futures
                .get(&future_id)
                .expect("future remains tracked")
                .state,
            super::FutureState::Ready(Value::Tagged { tag: 0, .. })
        ));
    }

    #[test]
    fn genserver_call_stop_error_returns_err_and_stops_worker() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                "Worker",
                RuntimeProcessKind::GenServer,
                RuntimeProcessInstance::Worker,
                false,
                0,
                0,
                None,
            )],
        };
        let mut vm = VM::new(bytecode);
        let pid = PidHandle {
            id: vm
                .allocate_supervised_worker(
                    "Worker".into(),
                    Some(Value::Int(int(41))),
                    "DynamicSupervisor".into(),
                )
                .expect("worker allocation should succeed"),
            process_name: "Worker".into(),
        };

        let value = vm
            .genserver_call_stop_error(&pid, vm.process_error("Boom", "boom"))
            .expect("stop error should return a result value");

        assert!(matches!(
            value,
            Value::Tagged { tag: 1, fields } if matches!(fields.first(), Some(Value::Error(err)) if err.kind == "Global::Boom")
        ));
        assert!(matches!(
            vm.process_runtime
                .processes
                .get(&pid.id)
                .expect("process exists")
                .status,
            super::ProcessStatus::Stopped
        ));
        assert_eq!(
            vm.process_runtime
                .processes
                .get(&pid.id)
                .expect("process exists")
                .state_value,
            None
        );
    }

    #[test]
    fn genserver_cast_stop_normal_stops_worker_and_clears_state() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                "Worker",
                RuntimeProcessKind::GenServer,
                RuntimeProcessInstance::Worker,
                false,
                0,
                0,
                None,
            )],
        };
        let mut vm = VM::new(bytecode);
        let pid = PidHandle {
            id: vm
                .allocate_supervised_worker(
                    "Worker".into(),
                    Some(Value::Int(int(41))),
                    "DynamicSupervisor".into(),
                )
                .expect("worker allocation should succeed"),
            process_name: "Worker".into(),
        };

        let value = vm
            .genserver_cast_stop_normal(&pid)
            .expect("cast stop should return ok unit");

        assert!(matches!(value, Value::Tagged { tag: 0, .. }));
        assert!(matches!(
            vm.process_runtime
                .processes
                .get(&pid.id)
                .expect("process exists")
                .status,
            super::ProcessStatus::Stopped
        ));
        assert_eq!(
            vm.process_runtime
                .processes
                .get(&pid.id)
                .expect("process exists")
                .state_value,
            None
        );
    }

    #[test]
    fn genserver_reply_later_commits_state_before_reply_resolves() {
        let mut bytecode = base_bytecode(vec![
            Opcode::Halt,
            Opcode::LoadConst(0),
            Opcode::LoadConst(1),
            Opcode::StructNew { field_count: 1 },
            Opcode::Return,
        ]);
        bytecode.constants = vec![Constant::Tag(0), Constant::Int(int(99))];
        bytecode.functions = vec![function_entry(0, 1, 0, 0, Some("Worker::callback"))];
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                "Worker",
                RuntimeProcessKind::GenServer,
                RuntimeProcessInstance::Worker,
                false,
                0,
                0,
                None,
            )],
        };
        let mut vm = VM::new(bytecode);
        let pid = PidHandle {
            id: vm
                .allocate_supervised_worker(
                    "Worker".into(),
                    Some(Value::Int(int(41))),
                    "DynamicSupervisor".into(),
                )
                .expect("worker allocation should succeed"),
            process_name: "Worker".into(),
        };

        let value = vm
            .genserver_call_reply_later(&pid, Value::Int(int(42)), vm.callable_for_function(0))
            .expect("reply later should start callback");

        let Value::PendingFuture(future_id) = value else {
            panic!("reply later should suspend on a future");
        };
        assert_eq!(
            vm.process_runtime
                .processes
                .get(&pid.id)
                .expect("process exists")
                .state_value,
            Some(Value::Int(int(42)))
        );
        assert!(vm.ready_future_value(future_id).is_none());
        vm.drive_ready_detached_tasks()
            .expect("reply callback should run under the scheduler");
        assert!(matches!(
            vm.ready_future_value(future_id),
            Some(Value::Tagged { tag: 0, fields }) if matches!(fields.first(), Some(Value::Int(value)) if *value == int(99))
        ));
    }

    #[test]
    fn genserver_reply_later_outer_timeout_wins_before_callback_sleep_completion() {
        let mut bytecode = base_bytecode(vec![Opcode::Halt]);
        bytecode.type_registry.register(TypeEntry {
            tag: 2,
            name: "Duration".into(),
            kind: TypeKind::Struct,
            field_names: vec!["millis".into()],
            private_flags: vec![true],
        });
        bytecode.runtime_process_specs = RuntimeProcessSpecTable {
            entries: vec![test_runtime_process_spec(
                0,
                "Worker",
                RuntimeProcessKind::GenServer,
                RuntimeProcessInstance::Worker,
                false,
                0,
                0,
                None,
            )],
        };
        let mut vm = VM::new(bytecode);
        let pid = PidHandle {
            id: vm
                .allocate_supervised_worker(
                    "Worker".into(),
                    Some(Value::Int(int(41))),
                    "DynamicSupervisor".into(),
                )
                .expect("worker allocation should succeed"),
            process_name: "Worker".into(),
        };
        let callback = Callable {
            target: CallableTarget::Builtin(builtin_id("__process_sleep")),
            lexical_captures: vec![Value::Tagged {
                tag: 2,
                fields: vec![Value::Int(int(100))],
            }],
            metadata: CallableMetadata::default(),
        };

        let value = vm
            .genserver_call_reply_later(&pid, Value::Int(int(42)), callback)
            .expect("reply later should create a reply future");
        let Value::PendingFuture(reply_future) = value else {
            panic!("reply later should return a pending reply future");
        };
        vm.process_runtime.attach_future_deadline(
            reply_future,
            vm.process_runtime.current_tick_ms,
            1,
            true,
        );

        vm.wait_for_any_future(&[reply_future])
            .expect("outer timeout should resolve the reply future");

        assert!(matches!(
            vm.ready_future_value(reply_future),
            Some(Value::Tagged { tag: 1, fields }) if matches!(fields.first(), Some(Value::Error(err)) if err.kind == "Global::Timeout")
        ));
        assert!(vm.process_runtime.reply_table.is_empty());
        assert!(vm
            .process_runtime
            .deadline_queue
            .iter()
            .all(|entry| entry.future_id != reply_future));
        assert_eq!(
            vm.process_runtime
                .futures
                .get(&reply_future)
                .expect("reply future should remain tracked")
                .deadline_tick,
            None
        );

        vm.process_runtime.current_tick_ms = 100;
        vm.expire_process_deadlines(100);
        vm.drive_ready_detached_tasks()
            .expect("sleeping callback completion should not overwrite timed-out reply");

        assert!(matches!(
            vm.ready_future_value(reply_future),
            Some(Value::Tagged { tag: 1, fields }) if matches!(fields.first(), Some(Value::Error(err)) if err.kind == "Global::Timeout")
        ));
        assert!(vm.process_runtime.detached_tasks.is_empty());
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
            .map(|idx| {
                test_runtime_process_spec(
                    0,
                    format!("Singleton{idx}"),
                    RuntimeProcessKind::Agent,
                    RuntimeProcessInstance::Singleton,
                    false,
                    0,
                    0,
                    None,
                )
            })
            .collect::<Vec<_>>();
        specs.push(test_runtime_process_spec(
            0,
            "Worker",
            RuntimeProcessKind::Agent,
            RuntimeProcessInstance::Worker,
            false,
            1,
            1,
            None,
        ));
        bytecode.runtime_process_specs = RuntimeProcessSpecTable { entries: specs };
        bytecode.runtime_boot_plan.singletons = (0..singleton_count)
            .map(|idx| SingletonBootEntry {
                process_name: format!("Singleton{idx}"),
                init_timeout_ms: 5_000,
                source: BootEntrySource::ExplicitConfig,
            })
            .collect();

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
