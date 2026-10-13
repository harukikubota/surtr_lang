//! Dedicated-feature real-VM allocation/RSS boundary probe. This module uses
//! only metrics available in both the historical Vec and current Rc builders.
use super::*;
use crate::vm::process_continuation::{DetachedCompletion, DetachedTaskState};
use std::io::{BufRead, Write};

#[path = "metrics.rs"]
mod metrics;

const PHASES: usize = 15;
const COUNTED: bool = cfg!(feature = "vm-memory-count");

struct Options {
    tasks: usize,
    length: usize,
    payload: String,
    repeats: usize,
    copy_mode: String,
    verify: bool,
    barrier: bool,
}

impl Options {
    fn read() -> Self {
        fn positive(name: &str, default: usize) -> usize {
            match std::env::var(name) {
                Ok(value) => {
                    let number = value.parse().unwrap_or_else(|_| panic!("invalid {name}"));
                    assert!(number > 0, "{name} must be positive");
                    number
                }
                Err(std::env::VarError::NotPresent) => default,
                Err(error) => panic!("invalid {name}: {error}"),
            }
        }
        fn flag(name: &str) -> bool {
            match std::env::var(name) {
                Ok(value) if value == "1" => true,
                Ok(value) if value == "0" => false,
                Err(std::env::VarError::NotPresent) => false,
                other => panic!("invalid {name}: {other:?}"),
            }
        }
        let profile = flag("SURTR_VM_MEMORY_PROFILE");
        let payload = std::env::var("SURTR_VM_MEMORY_PAYLOAD").unwrap_or_else(|_| "int".into());
        let copy_mode =
            std::env::var("SURTR_VM_MEMORY_COPY_MODE").unwrap_or_else(|_| "shared".into());
        assert!(matches!(payload.as_str(), "int" | "string"));
        assert!(matches!(copy_mode.as_str(), "baseline" | "shared"));
        let options = Self {
            tasks: if profile {
                positive("SURTR_VM_MEMORY_TASKS", 32)
            } else {
                1
            },
            length: if profile {
                positive("SURTR_VM_MEMORY_LEN", 16384)
            } else {
                32
            },
            payload,
            repeats: if profile {
                positive("SURTR_VM_MEMORY_REPEATS", 1)
            } else {
                1
            },
            copy_mode,
            verify: !profile || flag("PROBE_VERIFY"),
            barrier: profile && flag("SURTR_VM_MEMORY_BARRIER"),
        };
        assert!(
            !options.verify || !options.barrier,
            "verification and RSS are separate processes"
        );
        assert!(
            !COUNTED || !options.barrier,
            "allocation counting and RSS are separate processes"
        );
        options
    }
}

struct Barrier {
    enabled: bool,
    input: std::io::StdinLock<'static>,
    output: std::io::StdoutLock<'static>,
    line: String,
}

impl Barrier {
    fn new(enabled: bool) -> Self {
        let mut barrier = Self {
            enabled,
            input: std::io::stdin().lock(),
            output: std::io::stdout().lock(),
            line: String::with_capacity(32),
        };
        // All IO initialization and the startup handshake precede the baseline.
        // libtest prints its test-name prefix without a newline under nocapture.
        writeln!(barrier.output).unwrap();
        if enabled {
            writeln!(barrier.output, "START {}", std::process::id()).unwrap();
            barrier.wait();
        }
        barrier.output.flush().unwrap();
        barrier
    }
    fn wait(&mut self) {
        self.output.flush().unwrap();
        self.line.clear();
        let read = self.input.read_line(&mut self.line).unwrap();
        assert!(read > 0, "RSS runner must acknowledge every barrier");
        assert_eq!(self.line, "go\n", "invalid RSS acknowledgement");
    }
    fn observe(&mut self, repeat: usize, phase: &str) {
        if self.enabled {
            writeln!(self.output, "READY {repeat} {phase}").unwrap();
            self.wait();
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Counters {
    copies: usize,
    pushes: usize,
    callback_calls: usize,
    input_visits: usize,
    finishes: usize,
}

fn counters() -> Counters {
    let value = flat_map_metrics();
    Counters {
        copies: value.builder_clones,
        pushes: value.builder_pushes,
        callback_calls: value.callback_calls,
        input_visits: value.input_visits,
        finishes: value.builder_finishes,
    }
}

struct Record {
    repeat: usize,
    phase: &'static str,
    elapsed_ns: u64,
    allocation: metrics::Metrics,
    construction: Counters,
}

fn phase<R>(
    options: &Options,
    records: &mut Vec<Record>,
    barrier: &mut Barrier,
    repeat: usize,
    name: &'static str,
    action: impl FnOnce() -> R,
) -> R {
    reset_flat_map_metrics();
    let before = metrics::begin();
    let started = (!options.verify).then(std::time::Instant::now);
    let result = action();
    let elapsed_ns = started.map_or(0, |start| start.elapsed().as_nanos() as u64);
    let after = metrics::snapshot();
    let allocation = metrics::interval(before, after);
    if COUNTED {
        if let Some(previous) = records.last().filter(|previous| previous.repeat == repeat) {
            assert_eq!(
                previous.allocation.live_end_bytes, allocation.live_start_bytes,
                "unrecorded live allocation between VM boundaries"
            );
        }
    }
    assert!(
        records.len() < records.capacity(),
        "records must be preallocated"
    );
    records.push(Record {
        repeat,
        phase: name,
        elapsed_ns,
        allocation,
        construction: counters(),
    });
    // Endpoint RSS is sampled while the workload owners are unchanged; the
    // handshake itself is outside the phase allocation/timing interval.
    barrier.observe(repeat, name);
    result
}

fn payload(index: usize, kind: &str) -> Value {
    match kind {
        "int" => Value::Int(int(index)),
        "string" => Value::Str(format!("{index:020}{}", "x".repeat(236))),
        _ => panic!("invalid payload"),
    }
}

fn prepare(tasks: usize, length: usize, kind: &str) -> (VM, Vec<FutureId>, usize) {
    let total = length.checked_add(2048).unwrap();
    let output = list((0..total).map(|index| payload(index, kind)));
    let (mut vm, mapper) = constant_mapper_vm(output);
    vm.frames[0].call_site = Some((0, 1));
    let mut futures = Vec::with_capacity(tasks);
    for _ in 0..tasks {
        let before = counters();
        let target = before.pushes.checked_add(length).unwrap();
        let mut computation = flat_map_callable();
        computation.lexical_captures = vec![list([Value::Unit]), Value::Callable(mapper.clone())];
        let id = vm.process_runtime.next_detached_task_id;
        vm.start_task(computation, TaskMode::Async, None).unwrap();
        let mut task = vm.process_runtime.detached_tasks.take(&id).unwrap();
        let DetachedCompletion::Future(Some(future)) = task.completion else {
            panic!("new async task must have a result Future")
        };
        futures.push(future);
        let DetachedTaskState::Runnable(context) = &mut task.state else {
            panic!("new task must be runnable")
        };
        let caller = vm.take_execution_context(vm.pc, ExecutionTarget::TopLevel);
        for _ in 0..length.checked_add(32).unwrap() {
            if counters().pushes == target {
                break;
            }
            let outcome = vm.run_quantum(context, &mut Budget::new(1));
            assert!(
                matches!(outcome, ProcessRunOutcome::QuantumExpired),
                "{outcome:?}"
            );
        }
        let after = counters();
        assert_eq!(after.pushes, target);
        assert_eq!(after.callback_calls - before.callback_calls, 1);
        assert_eq!(after.finishes - before.finishes, 0);
        assert_eq!(after.copies - before.copies, 0);
        vm.restore_execution_context(caller);
        task.test_mark_started();
        vm.process_runtime.detached_tasks.insert(id, task);
        assert!(vm.ready_future_value(future).is_none());
    }
    assert_eq!(vm.process_runtime.detached_tasks.len(), tasks);
    (vm, futures, total)
}

fn verify(vm: &VM, futures: &[FutureId], total: usize, kind: &str) {
    assert!(vm.process_runtime.detached_tasks.is_empty());
    for &future in futures {
        let Some(Value::List(mut output)) = vm.ready_future_value(future) else {
            panic!("completed Task must deliver its List Future")
        };
        assert_eq!(output.len(), total);
        for index in 0..total {
            assert_eq!(output.head_value(), Some(payload(index, kind)));
            output = output.tail_handle().unwrap();
        }
        assert!(output.head_value().is_none());
    }
}

fn assert_record(record: &Record, copies: usize, pushes: usize, callbacks: usize, finishes: usize) {
    let c = record.construction;
    assert_eq!(c.copies, copies, "{}", record.phase);
    assert_eq!(c.pushes, pushes, "{}", record.phase);
    assert_eq!(c.callback_calls, callbacks, "{}", record.phase);
    assert_eq!(c.input_visits, callbacks, "{}", record.phase);
    assert_eq!(c.finishes, finishes, "{}", record.phase);
}

fn repeat(options: &Options, records: &mut Vec<Record>, barrier: &mut Barrier, index: usize) {
    let baseline = metrics::snapshot();
    let (mut vm, futures, total) =
        phase(options, records, barrier, index, "prepare_shared", || {
            prepare(options.tasks, options.length, &options.payload)
        });
    assert_record(
        records.last().unwrap(),
        0,
        options.tasks * options.length,
        options.tasks,
        0,
    );
    let original = phase(options, records, barrier, index, "save", || checkpoint(&vm));
    assert_record(records.last().unwrap(), 0, 0, 0, 0);
    let unchanged = phase(options, records, barrier, index, "save_unchanged", || {
        checkpoint(&vm)
    });
    assert_record(records.last().unwrap(), 0, 0, 0, 0);
    phase(options, records, barrier, index, "drop_unchanged", || {
        drop(unchanged)
    });
    assert_record(records.last().unwrap(), 0, 0, 0, 0);
    for (name, clones) in [("first_resume", options.tasks), ("second_resume", 0)] {
        phase(options, records, barrier, index, name, || {
            vm.drive_ready_runtime_quantum()
        })
        .unwrap();
        assert_record(records.last().unwrap(), clones, options.tasks * 256, 0, 0);
    }
    let advanced = phase(options, records, barrier, index, "resave", || {
        checkpoint(&vm)
    });
    assert_record(records.last().unwrap(), 0, 0, 0, 0);
    phase(
        options,
        records,
        barrier,
        index,
        "resume_after_resave",
        || vm.drive_ready_runtime_quantum(),
    )
    .unwrap();
    assert_record(
        records.last().unwrap(),
        options.tasks,
        options.tasks * 256,
        0,
        0,
    );
    phase(options, records, barrier, index, "rollback", || {
        vm.rollback_to_checkpoint(advanced)
    });
    assert_record(records.last().unwrap(), 0, 0, 0, 0);
    phase(options, records, barrier, index, "complete_shared", || {
        vm.drain_background_tasks()
    })
    .unwrap();
    assert_record(
        records.last().unwrap(),
        0,
        options.tasks * 1536,
        0,
        options.tasks,
    );
    if options.verify {
        verify(&vm, &futures, total, &options.payload);
    }
    phase(options, records, barrier, index, "drop_changed", || {
        drop(original)
    });
    assert_record(records.last().unwrap(), 0, 0, 0, 0);
    phase(options, records, barrier, index, "drop_shared_vm", || {
        drop(vm);
        drop(futures);
    });
    assert_record(records.last().unwrap(), 0, 0, 0, 0);
    metrics::restored(baseline, metrics::snapshot());

    let unique_baseline = metrics::snapshot();
    let (mut unique, unique_futures, unique_total) =
        phase(options, records, barrier, index, "prepare_unique", || {
            prepare(options.tasks, options.length, &options.payload)
        });
    assert_eq!(unique_total, total);
    assert_record(
        records.last().unwrap(),
        0,
        options.tasks * options.length,
        options.tasks,
        0,
    );
    phase(options, records, barrier, index, "unique_complete", || {
        unique.drain_background_tasks()
    })
    .unwrap();
    assert_record(
        records.last().unwrap(),
        0,
        options.tasks * 2048,
        0,
        options.tasks,
    );
    if options.verify {
        verify(&unique, &unique_futures, total, &options.payload);
    }
    phase(options, records, barrier, index, "drop_unique_vm", || {
        drop(unique);
        drop(unique_futures);
    });
    assert_record(records.last().unwrap(), 0, 0, 0, 0);
    metrics::restored(unique_baseline, metrics::snapshot());
    metrics::restored(baseline, metrics::snapshot());
}

#[test]
fn vm_memory_profile() {
    let options = Options::read();
    let mut records = Vec::with_capacity(options.repeats.checked_mul(PHASES).unwrap());
    // Explicit small warmup initializes VM/helper/metric state before the measured
    // lifetime. It is the same for both variants and is part of process RSS scope.
    reset_flat_map_metrics();
    let (mut warmup, warmup_futures, total) = prepare(1, 1, "int");
    warmup.drain_background_tasks().unwrap();
    verify(&warmup, &warmup_futures, total, "int");
    drop(warmup);
    drop(warmup_futures);
    reset_flat_map_metrics();
    let _ = counters();
    let _ = std::time::Instant::now();
    let mut barrier = Barrier::new(options.barrier);
    let baseline = metrics::snapshot();
    for index in 0..options.repeats {
        repeat(&options, &mut records, &mut barrier, index);
    }
    metrics::restored(baseline, metrics::snapshot());
    assert_eq!(records.len(), options.repeats * PHASES);
    // All JSON construction, writing and record destruction follow all VM drops.
    if options.verify {
        writeln!(barrier.output, "{}", serde_json::json!({
            "profile": "checkpoint_builder_vm_memory_verify", "tasks": options.tasks,
            "builder_len": options.length, "output_len": options.length + 2048,
            "payload": options.payload, "repeats": options.repeats, "copy_mode": options.copy_mode,
            "allocator_counting": COUNTED, "verified": true, "workload_live_restored": COUNTED,
        })).unwrap();
    } else {
        let counted = |value: usize| {
            if COUNTED {
                serde_json::json!(value)
            } else {
                serde_json::Value::Null
            }
        };
        for record in records {
            let m = record.allocation;
            let c = record.construction;
            writeln!(barrier.output, "{}", serde_json::json!({
                "profile": "checkpoint_builder_vm_memory", "tasks": options.tasks,
                "builder_len": options.length, "output_len": options.length + 2048,
                "payload": options.payload, "repeats": options.repeats, "copy_mode": options.copy_mode,
                "allocator_counting": COUNTED, "repeat": record.repeat, "phase": record.phase,
                "elapsed_ns": record.elapsed_ns,
                "allocations": counted(m.allocations), "reallocations": counted(m.reallocations),
                "requested_bytes": counted(m.requested_bytes), "freed_bytes": counted(m.freed_bytes),
                "live_start_bytes": counted(m.live_start_bytes), "live_end_bytes": counted(m.live_end_bytes),
                "live_delta_bytes": if COUNTED { serde_json::json!(m.live_end_bytes as i128 - m.live_start_bytes as i128) } else { serde_json::Value::Null },
                "peak_extra_bytes": counted(m.peak_extra_bytes),
                "workload_live_baseline_bytes": counted(metrics::live(baseline)),
                "copies": c.copies, "pushes": c.pushes, "callback_calls": c.callback_calls,
                "input_visits": c.input_visits, "finishes": c.finishes,
            })).unwrap();
        }
    }
    barrier.output.flush().unwrap();
}
