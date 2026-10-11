//! Requested allocation sizes and wall time; no clone/copy counters or thresholds.
#[cfg(feature = "count-alloc")]
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

#[cfg(feature = "count-alloc")]
struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "count-alloc")]
fn allocated(size: usize) {
    let live = LIVE.fetch_add(size, Relaxed) + size;
    PEAK.fetch_max(live, Relaxed);
    REQUESTED.fetch_add(size, Relaxed);
}
#[cfg(feature = "count-alloc")]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            ALLOCS.fetch_add(1, Relaxed);
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            ALLOCS.fetch_add(1, Relaxed);
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        FREED.fetch_add(layout.size(), Relaxed);
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: Layout, size: usize) -> *mut u8 {
        let new = System.realloc(ptr, old, size);
        if !new.is_null() {
            REALLOCS.fetch_add(1, Relaxed);
            LIVE.fetch_sub(old.size(), Relaxed);
            FREED.fetch_add(old.size(), Relaxed);
            allocated(size);
        }
        new
    }
}
#[cfg(feature = "count-alloc")]
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy)]
struct Snapshot {
    live: usize,
    allocs: usize,
    reallocs: usize,
    requested: usize,
    freed: usize,
}
fn snapshot() -> Snapshot {
    Snapshot {
        live: LIVE.load(Relaxed),
        allocs: ALLOCS.load(Relaxed),
        reallocs: REALLOCS.load(Relaxed),
        requested: REQUESTED.load(Relaxed),
        freed: FREED.load(Relaxed),
    }
}
#[derive(Default)]
pub struct Metrics {
    pub ns: u64,
    pub allocations: usize,
    pub reallocations: usize,
    pub requested_bytes: usize,
    pub freed_bytes: usize,
    pub live_delta_bytes: i64,
    pub peak_extra_bytes: usize,
}

pub fn measured<T>(f: impl FnOnce() -> T) -> (T, Metrics) {
    let before = snapshot();
    PEAK.store(before.live, Relaxed);
    let started = Instant::now();
    let result = black_box(f());
    let ns = started.elapsed().as_nanos();
    let after = snapshot();
    let peak = PEAK.load(Relaxed);
    (
        result,
        Metrics {
            ns: ns.try_into().expect("measurement duration fits u64"),
            allocations: after.allocs - before.allocs,
            reallocations: after.reallocs - before.reallocs,
            requested_bytes: after.requested - before.requested,
            freed_bytes: after.freed - before.freed,
            live_delta_bytes: after.live as i64 - before.live as i64,
            peak_extra_bytes: peak - before.live,
        },
    )
}

pub fn emit(
    case: &str,
    phase: &str,
    n: usize,
    kind: &str,
    metrics: Metrics,
    details: serde_json::Value,
) {
    let mut stats = serde_json::json!({
        "case": case, "phase": phase, "n": n, "payload": kind,
        "ns": metrics.ns,
        "allocator_counting": cfg!(feature = "count-alloc"),
        "correctness_verification": std::env::var_os("PROBE_VERIFY").is_some(),
    });
    let counted = cfg!(feature = "count-alloc");
    for (name, value) in [
        ("allocations", metrics.allocations),
        ("reallocations", metrics.reallocations),
        ("requested_bytes", metrics.requested_bytes),
        ("freed_bytes", metrics.freed_bytes),
        ("peak_extra_bytes", metrics.peak_extra_bytes),
    ] {
        stats[name] = if counted {
            value.into()
        } else {
            serde_json::Value::Null
        };
    }
    stats["live_delta_bytes"] = if counted {
        metrics.live_delta_bytes.into()
    } else {
        serde_json::Value::Null
    };
    let fields = details
        .as_object()
        .expect("metric details must be an object");
    for (key, value) in fields {
        assert!(
            !stats.as_object().unwrap().contains_key(key),
            "duplicate metric field {key}"
        );
        stats[key] = value.clone();
    }
    println!("{stats}");
}
