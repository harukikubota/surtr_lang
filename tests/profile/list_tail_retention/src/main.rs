//! RT-2 manual models. Not a production representation or performance gate.
use sindr::primitives::int;
use sindr::runtime::{ListHandle, Value};
#[cfg(feature = "count-alloc")]
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

#[derive(Default)]
struct Metrics {
    ns: u64,
    allocations: usize,
    reallocations: usize,
    requested_bytes: usize,
    freed_bytes: usize,
    live_delta_bytes: i64,
    peak_extra_bytes: usize,
}
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
fn measured<T>(f: impl FnOnce() -> T) -> (T, Metrics) {
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
            ns: ns as u64,
            allocations: after.allocs - before.allocs,
            reallocations: after.reallocs - before.reallocs,
            requested_bytes: after.requested - before.requested,
            freed_bytes: after.freed - before.freed,
            live_delta_bytes: after.live as i64 - before.live as i64,
            peak_extra_bytes: peak - before.live,
        },
    )
}

const CHUNK_SIZE: usize = 256;

// Forward ownership: retaining a suffix does not retain nodes before it.
struct Chunk {
    items: Vec<Value>,
    next: Option<Rc<Chunk>>,
}
impl Drop for Chunk {
    fn drop(&mut self) {
        self.items.clear();
        let mut next = self.next.take();
        while let Some(node) = next {
            match Rc::try_unwrap(node) {
                Ok(mut owned) => {
                    owned.items.clear();
                    next = owned.next.take();
                }
                Err(_) => break,
            }
        }
    }
}
#[derive(Clone)]
struct ChunkCursor {
    node: Option<Rc<Chunk>>,
    offset: usize,
    len: usize,
}
impl ChunkCursor {
    fn build(n: usize, kind: &str) -> Self {
        let mut node = None;
        for start in (0..n).step_by(CHUNK_SIZE).rev() {
            let end = start.checked_add(CHUNK_SIZE).unwrap().min(n);
            let items = (start..end).map(|i| payload(kind, i)).collect();
            node = Some(Rc::new(Chunk { items, next: node }));
        }
        Self {
            node,
            offset: 0,
            len: n,
        }
    }
    fn tail(&self) -> Option<Self> {
        let current = self.node.as_ref()?;
        let len = self.len.checked_sub(1).unwrap();
        let offset = self.offset.checked_add(1).unwrap();
        Some(if offset == current.items.len() {
            Self {
                node: current.next.clone(),
                offset: 0,
                len,
            }
        } else {
            Self {
                node: self.node.clone(),
                offset,
                len,
            }
        })
    }
    fn head(&self) -> Option<Value> {
        self.node
            .as_ref()
            .map(|node| node.items[self.offset].clone())
    }
}
#[derive(Clone)]
enum Cursor {
    Packed(ListHandle),
    Chunked(ChunkCursor),
}
impl Cursor {
    fn build(mode: &str, n: usize, kind: &str) -> Self {
        match mode {
            "shared" | "compact" => Self::Packed(ListHandle::from_items(
                (0..n).map(|i| payload(kind, i)).collect(),
            )),
            "chunk" => Self::Chunked(ChunkCursor::build(n, kind)),
            _ => panic!("unknown mode"),
        }
    }
    fn len(&self) -> usize {
        match self {
            Self::Packed(list) => list.len(),
            Self::Chunked(list) => list.len,
        }
    }
    fn tail(&self) -> Option<Self> {
        match self {
            Self::Packed(list) => list.tail_handle().map(Self::Packed),
            Self::Chunked(list) => list.tail().map(Self::Chunked),
        }
    }
    fn head(&self) -> Option<Value> {
        match self {
            Self::Packed(list) => list.head_value(),
            Self::Chunked(list) => list.head(),
        }
    }
    fn compact(self) -> Self {
        let Self::Packed(list) = self else {
            panic!("compaction requires Packed model")
        };
        Self::Packed(ListHandle::from_items(list.iter().collect()))
    }
    fn verify(&self, kind: &str, start: usize, n: usize) {
        assert_eq!(self.len(), n - start);
        let mut cursor = self.clone();
        for i in start..n {
            assert_eq!(cursor.head(), Some(payload(kind, i)), "element {i}");
            cursor = cursor.tail().unwrap();
        }
        assert_eq!(cursor.len(), 0);
        assert!(cursor.head().is_none());
        assert!(cursor.tail().is_none());
    }
}
fn payload(kind: &str, i: usize) -> Value {
    match kind {
        "int" => Value::Int(int(i)),
        "string" | "nested" => {
            let mut text = format!("{i:010}:");
            text.push_str(&"x".repeat(256 - text.len()));
            let mut value = Value::Str(text);
            if kind == "nested" {
                for _ in 0..3 {
                    value = Value::List(ListHandle::from_items(vec![value]));
                }
            }
            value
        }
        _ => panic!("unknown payload"),
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 5, "mode n payload ownership");
    let mode = args[1].as_str();
    let n: usize = args[2].parse().unwrap();
    let kind = args[3].as_str();
    let ownership = args[4].as_str();
    assert!(n >= 2);
    assert!(matches!(ownership, "suffix" | "siblings" | "checkpoint"));
    let verify = std::env::var_os("PROBE_VERIFY").is_some();
    let (root, build) = measured(|| Cursor::build(mode, n, kind));
    let ((mut tail, sibling, checkpoint), advance) = measured(|| {
        let checkpoint = (ownership == "checkpoint").then(|| root.clone());
        let mut sibling = None;
        let mut tail = root.clone();
        for step in 1..n {
            tail = tail.tail().unwrap();
            if ownership == "siblings" && step == n / 2 {
                sibling = Some(tail.clone());
            }
        }
        (tail, sibling, checkpoint)
    });
    assert_eq!(tail.len(), 1);
    assert_eq!(advance.allocations, 0, "tail traversal must not allocate");
    assert_eq!(advance.reallocations, 0);
    let (_, compact) = measured(|| {
        if mode == "compact" {
            tail = tail.clone().compact();
        }
    });
    // Verification is intentionally a separate process, never performance data.
    if verify {
        root.verify(kind, 0, n);
        tail.verify(kind, n - 1, n);
        if let Some(value) = &sibling {
            value.verify(kind, n / 2, n);
        }
        if let Some(value) = &checkpoint {
            value.verify(kind, 0, n);
        }
    }
    let (_, drop_root) = measured(|| drop(root));
    if verify {
        tail.verify(kind, n - 1, n);
        if let Some(value) = &sibling {
            value.verify(kind, n / 2, n);
        }
        if let Some(value) = &checkpoint {
            value.verify(kind, 0, n);
        }
    }
    let (_, drop_sibling) = measured(|| drop(sibling));
    let (_, drop_checkpoint) = measured(|| drop(checkpoint));
    if verify {
        tail.verify(kind, n - 1, n);
    }
    let (_, drop_last) = measured(|| drop(tail));
    let phases = [
        ("build", build),
        ("advance", advance),
        ("compact", compact),
        ("drop_root", drop_root),
        ("drop_sibling", drop_sibling),
        ("drop_checkpoint", drop_checkpoint),
        ("drop_last", drop_last),
    ];
    let mut retained = 0i64;
    for (phase, m) in phases {
        retained += m.live_delta_bytes;
        println!(
            "{}",
            serde_json::json!({
                "mode": mode, "n": n, "payload": kind, "ownership": ownership,
                "phase": phase, "chunk_capacity": CHUNK_SIZE,
                "allocator_counting": cfg!(feature="count-alloc"), "verification": verify,
                "ns": m.ns, "allocations": m.allocations, "reallocations": m.reallocations,
                "requested_bytes": m.requested_bytes, "freed_bytes": m.freed_bytes,
                "live_delta_bytes": m.live_delta_bytes, "peak_extra_bytes": m.peak_extra_bytes,
                "workload_retained_bytes": retained,
            })
        );
    }
    if cfg!(feature = "count-alloc") {
        assert_eq!(retained, 0, "workload allocations must be released");
    }
}
