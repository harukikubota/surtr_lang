//! Standalone release probe; imports the exact private source under measurement.
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
    assert_eq!(args.len(), 5, "case length clone-count mode");
    let case = &args[1];
    let n: usize = args[2].parse().unwrap();
    let count: usize = args[3].parse().unwrap();
    assert!(count > 0);
    let mode = &args[4];
    assert!(matches!(mode.as_str(), "allocation" | "time" | "verify"));
    let mut original: OperandStack = vec![Value::Unit; n].into();
    match case.as_str() {
        "never" => {},
        "cleared" | "popped" | "truncated" | "resolved" => {
            original.push(Value::PendingFuture(1));
            match case.as_str() {
                "cleared" => original.clear(),
                "popped" => { original.pop(); },
                "truncated" => original.truncate(n),
                "resolved" => original.replace_ready_futures(&[(1, Value::Unit)].into()),
                _ => unreachable!(),
            }
        },
        "pending" => {
            original.push(Value::PendingFuture(1));
            original.push(Value::PendingFuture(1));
        },
        _ => panic!("unknown case"),
    }
    let mut clones: Vec<OperandStack> = Vec::with_capacity(count);
    if mode == "allocation" { start(0); }
    let clock = (mode == "time").then(Instant::now);
    for _ in 0..count {
        clones.push(OperandStack::clone(black_box(&original)));
    }
    black_box(&clones);
    let clone_ns = clock.map_or(0, |t| t.elapsed().as_nanos());
    let clone_stats = if mode == "allocation" { stop() } else { Stats::default() };
    if mode == "verify" {
        for saved in &clones {
            assert_eq!(saved, &original);
            assert!(saved.iter().eq(original.iter()));
            assert_eq!(saved.pending_future_ids().collect::<Vec<_>>(), original.pending_future_ids().collect::<Vec<_>>());
        }
    }
    if mode == "allocation" { start(clone_stats.live); }
    let clock = (mode == "time").then(Instant::now);
    // Root vector storage was allocated before the measurement and stays alive.
    clones.clear();
    let drop_ns = clock.map_or(0, |t| t.elapsed().as_nanos());
    let drop_stats = if mode == "allocation" { stop() } else { Stats::default() };
    if mode == "allocation" {
        assert_eq!(drop_stats.live, 0, "all counted clone allocations must be freed");
        assert_eq!(clone_stats.requested, drop_stats.freed);
        assert_eq!(clone_stats.freed, 0);
        assert_eq!(clone_stats.reallocs, 0);
    }
    if mode == "verify" {
        println!("{{\"case\":\"{case}\",\"n\":{n},\"count\":{count},\"mode\":\"verify\",\"verified\":true}}");
    } else {
        for (phase, elapsed_ns, stats) in [("clone", clone_ns, clone_stats), ("drop", drop_ns, drop_stats)] {
            println!("{{\"case\":\"{case}\",\"n\":{n},\"count\":{count},\"mode\":\"{mode}\",\"phase\":\"{phase}\",\"elapsed_ns\":{elapsed_ns},\"alloc_calls\":{},\"realloc_calls\":{},\"requested_bytes\":{},\"freed_bytes\":{},\"live_bytes\":{},\"peak_bytes\":{},\"value_bytes\":{},\"stack_bytes\":{},\"metadata_bytes\":{},\"position_bytes\":{}}}", stats.allocs, stats.reallocs, stats.requested, stats.freed, stats.live, stats.peak, std::mem::size_of::<Value>(), std::mem::size_of::<OperandStack>(), std::mem::size_of::<Vec<usize>>(), std::mem::size_of::<usize>());
        }
    }
}
