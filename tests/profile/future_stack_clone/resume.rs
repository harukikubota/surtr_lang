//! Standalone metadata construction, clone, first mutation and drop probe.
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::time::Instant;
use sindr::runtime::Value;

#[allow(dead_code)]
mod stack {
    include!(env!("SURTR_OPERAND_STACK_SOURCE"));
}
use stack::OperandStack;

struct MeasuredSystem;
static ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn allocated(bytes: usize) {
    REQUESTED.fetch_add(bytes, Relaxed);
    let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(live, Relaxed);
}
unsafe impl GlobalAlloc for MeasuredSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() && ENABLED.load(Relaxed) {
            ALLOCS.fetch_add(1, Relaxed);
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ENABLED.load(Relaxed) {
            FREED.fetch_add(layout.size(), Relaxed);
            LIVE.fetch_sub(layout.size(), Relaxed);
        }
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: Layout, size: usize) -> *mut u8 {
        let result = System.realloc(ptr, old, size);
        if !result.is_null() && ENABLED.load(Relaxed) {
            REALLOCS.fetch_add(1, Relaxed);
            FREED.fetch_add(old.size(), Relaxed);
            LIVE.fetch_sub(old.size(), Relaxed);
            allocated(size);
        }
        result
    }
}
#[global_allocator]
static ALLOCATOR: MeasuredSystem = MeasuredSystem;

#[derive(Default)]
struct Stats { allocs: usize, reallocs: usize, requested: usize, freed: usize, live: usize, peak: usize }
fn start(initial: usize) {
    ALLOCS.store(0, Relaxed); REALLOCS.store(0, Relaxed);
    REQUESTED.store(0, Relaxed); FREED.store(0, Relaxed);
    LIVE.store(initial, Relaxed); PEAK.store(initial, Relaxed);
    ENABLED.store(true, Relaxed);
}
fn stop() -> Stats {
    ENABLED.store(false, Relaxed);
    Stats { allocs: ALLOCS.load(Relaxed), reallocs: REALLOCS.load(Relaxed),
        requested: REQUESTED.load(Relaxed), freed: FREED.load(Relaxed),
        live: LIVE.load(Relaxed), peak: PEAK.load(Relaxed) }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 6, "ordinary-count pending-count clone-count mode operation");
    let n: usize = args[1].parse().unwrap();
    let pending: usize = args[2].parse().unwrap();
    let count: usize = args[3].parse().unwrap();
    assert!(pending > 0 && count > 0);
    let mode = &args[4]; let operation = &args[5];
    assert!(matches!(mode.as_str(), "allocation" | "time" | "verify"));
    assert!(matches!(operation.as_str(), "none" | "append" | "resolve"));
    let mut values = vec![Value::Unit; n + pending + 1];
    values.truncate(n);
    let mut original: OperandStack = values.into();
    let mut clones: Vec<OperandStack> = Vec::with_capacity(count);
    let ready = [(1, Value::Unit)].into();
    if mode == "allocation" { start(0); }
    let clock = (mode == "time").then(Instant::now);
    for _ in 0..pending { original.push(Value::PendingFuture(1)); }
    let construct_ns = clock.map_or(0, |t| t.elapsed().as_nanos());
    let construct = if mode == "allocation" { stop() } else { Stats::default() };
    if mode == "allocation" { start(0); }
    let clock = (mode == "time").then(Instant::now);
    for _ in 0..count { clones.push(OperandStack::clone(black_box(&original))); }
    black_box(&clones);
    let clone_ns = clock.map_or(0, |t| t.elapsed().as_nanos());
    let cloned = if mode == "allocation" { stop() } else { Stats::default() };
    if mode == "verify" { for saved in &clones { assert_eq!(saved, &original); } }
    if mode == "allocation" { start(cloned.live); }
    let clock = (mode == "time").then(Instant::now);
    for saved in &mut clones {
        match operation.as_str() {
            "none" => {},
            "append" => saved.push(Value::PendingFuture(2)),
            "resolve" => saved.replace_ready_futures(&ready),
            _ => unreachable!(),
        }
    }
    black_box(&clones);
    let mutate_ns = clock.map_or(0, |t| t.elapsed().as_nanos());
    let mutated = if mode == "allocation" { stop() } else { Stats::default() };
    if mode == "verify" {
        assert_eq!(original.pending_future_ids().count(), pending);
        assert!(original.pending_future_ids().all(|id| id == 1));
        for saved in &clones {
            match operation.as_str() {
                "none" => assert_eq!(saved, &original),
                "append" => {
                    assert!(saved[..original.len()].iter().eq(original.iter()));
                    assert_eq!(saved.last(), Some(&Value::PendingFuture(2)));
                    assert_eq!(saved.pending_future_ids().count(), pending + 1);
                },
                "resolve" => {
                    assert!(saved.iter().all(|v| *v == Value::Unit));
                    assert_eq!(saved.pending_future_ids().count(), 0);
                },
                _ => unreachable!(),
            }
        }
    }
    if mode == "allocation" { start(mutated.live); }
    let clock = (mode == "time").then(Instant::now);
    clones.clear();
    let drop_ns = clock.map_or(0, |t| t.elapsed().as_nanos());
    let dropped = if mode == "allocation" { stop() } else { Stats::default() };
    if mode == "allocation" {
        assert_eq!(dropped.live, 0, "counted clone/mutation storage must all be freed");
        assert_eq!(cloned.requested + mutated.requested, cloned.freed + mutated.freed + dropped.freed);
        assert_eq!(dropped.requested, 0);
    }
    if mode == "verify" {
        println!("{{\"n\":{n},\"pending\":{pending},\"count\":{count},\"operation\":\"{operation}\",\"mode\":\"verify\",\"verified\":true}}");
    } else {
        for (phase, elapsed_ns, stats) in [("construct", construct_ns, construct), ("clone", clone_ns, cloned), ("mutate", mutate_ns, mutated), ("drop", drop_ns, dropped)] {
            println!("{{\"n\":{n},\"pending\":{pending},\"count\":{count},\"operation\":\"{operation}\",\"mode\":\"{mode}\",\"phase\":\"{phase}\",\"elapsed_ns\":{elapsed_ns},\"alloc_calls\":{},\"realloc_calls\":{},\"requested_bytes\":{},\"freed_bytes\":{},\"live_bytes\":{},\"peak_bytes\":{},\"value_bytes\":{},\"stack_bytes\":{},\"metadata_bytes\":{},\"position_bytes\":{}}}", stats.allocs, stats.reallocs, stats.requested, stats.freed, stats.live, stats.peak, std::mem::size_of::<Value>(), std::mem::size_of::<OperandStack>(), std::mem::size_of::<Vec<usize>>(), std::mem::size_of::<usize>());
        }
    }
}
