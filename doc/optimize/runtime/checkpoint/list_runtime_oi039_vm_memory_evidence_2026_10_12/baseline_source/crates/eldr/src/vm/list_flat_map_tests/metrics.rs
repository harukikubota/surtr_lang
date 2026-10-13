//! System allocation requests, not allocator usable sizes or physical RSS.
//! The thread ledger has constant initialization and no destructor. Allocator
//! callbacks only access integer Cells/atomics; they never format, lock or allocate.

#[cfg(feature = "vm-memory-count")]
mod counted {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    #[derive(Clone, Copy, Default)]
    pub(super) struct Ledger {
        pub(super) allocations: usize,
        pub(super) reallocations: usize,
        pub(super) requested: usize,
        pub(super) freed: usize,
        // Other-thread allocations can be transferred to this thread by the
        // harness before measurement. Its cumulative balance may be negative.
        pub(super) live: i128,
        pub(super) peak: i128,
    }

    thread_local! {
        static THREAD: Cell<Ledger> = const { Cell::new(Ledger {
            allocations: 0, reallocations: 0, requested: 0, freed: 0,
            live: 0, peak: 0,
        }) };
    }

    static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
    static REALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
    static REQUESTED: AtomicUsize = AtomicUsize::new(0);
    static FREED: AtomicUsize = AtomicUsize::new(0);
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    static TLS_FAILURES: AtomicUsize = AtomicUsize::new(0);

    fn thread_event(allocated: usize, freed: usize, reallocated: bool, allocation: bool) {
        if THREAD
            .try_with(|cell| {
                let mut ledger = cell.get();
                ledger.allocations = ledger.allocations.wrapping_add(usize::from(allocation));
                ledger.reallocations = ledger.reallocations.wrapping_add(usize::from(reallocated));
                ledger.requested = ledger.requested.wrapping_add(allocated);
                ledger.freed = ledger.freed.wrapping_add(freed);
                ledger.live = ledger
                    .live
                    .wrapping_sub(freed as i128)
                    .wrapping_add(allocated as i128);
                ledger.peak = ledger.peak.max(ledger.live);
                cell.set(ledger);
            })
            .is_err()
        {
            // Global accounting remains complete during TLS teardown. A failed
            // TLS access inside a measured interval invalidates that interval.
            TLS_FAILURES.fetch_add(1, Relaxed);
        }
    }

    fn allocated(size: usize, reallocated: bool, freed: usize) {
        if reallocated {
            REALLOCATIONS.fetch_add(1, Relaxed);
            LIVE.fetch_sub(freed, Relaxed);
            FREED.fetch_add(freed, Relaxed);
        } else {
            ALLOCATIONS.fetch_add(1, Relaxed);
        }
        let live = LIVE.fetch_add(size, Relaxed).wrapping_add(size);
        REQUESTED.fetch_add(size, Relaxed);
        PEAK.fetch_max(live, Relaxed);
        thread_event(size, freed, reallocated, !reallocated);
    }

    struct CountingAllocator;

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = System.alloc(layout);
            if !pointer.is_null() {
                allocated(layout.size(), false, 0);
            }
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = System.alloc_zeroed(layout);
            if !pointer.is_null() {
                allocated(layout.size(), false, 0);
            }
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            LIVE.fetch_sub(layout.size(), Relaxed);
            FREED.fetch_add(layout.size(), Relaxed);
            thread_event(0, layout.size(), false, false);
            System.dealloc(pointer, layout);
        }

        unsafe fn realloc(&self, pointer: *mut u8, old: Layout, size: usize) -> *mut u8 {
            let replacement = System.realloc(pointer, old, size);
            if !replacement.is_null() {
                // Successful realloc retires the old requested size and installs
                // the new one. This does not measure old/new physical overlap.
                allocated(size, true, old.size());
            }
            replacement
        }
    }

    #[cfg(all(test, feature = "vm-memory-count"))]
    #[global_allocator]
    static ALLOCATOR: CountingAllocator = CountingAllocator;

    #[derive(Clone, Copy)]
    pub(in super::super) struct Snapshot {
        global: Ledger,
        thread: Ledger,
        tls_failures: usize,
    }

    pub(in super::super) fn snapshot() -> Snapshot {
        Snapshot {
            global: Ledger {
                allocations: ALLOCATIONS.load(Relaxed),
                reallocations: REALLOCATIONS.load(Relaxed),
                requested: REQUESTED.load(Relaxed),
                freed: FREED.load(Relaxed),
                live: LIVE.load(Relaxed) as i128,
                peak: PEAK.load(Relaxed) as i128,
            },
            thread: THREAD
                .try_with(Cell::get)
                .expect("measurement thread TLS must be accessible"),
            tls_failures: TLS_FAILURES.load(Relaxed),
        }
    }

    pub(in super::super) fn begin() -> Snapshot {
        let before = snapshot();
        PEAK.store(before.global.live as usize, Relaxed);
        THREAD.with(|cell| {
            let mut ledger = cell.get();
            ledger.peak = ledger.live;
            cell.set(ledger);
        });
        before
    }

    pub(in super::super) fn interval(before: Snapshot, after: Snapshot) -> super::Metrics {
        assert_eq!(
            after.tls_failures, before.tls_failures,
            "allocator TLS unavailable during measurement"
        );
        let g = after.global;
        let b = before.global;
        let t = after.thread;
        let s = before.thread;
        let global = (
            g.allocations - b.allocations,
            g.reallocations - b.reallocations,
            g.requested - b.requested,
            g.freed - b.freed,
            g.live - b.live,
        );
        let thread = (
            t.allocations - s.allocations,
            t.reallocations - s.reallocations,
            t.requested - s.requested,
            t.freed - s.freed,
            t.live - s.live,
        );
        assert_eq!(
            global, thread,
            "other-thread allocation activity contaminated the workload"
        );
        assert_eq!(
            global.2 as i128 - global.3 as i128,
            global.4,
            "requested-live balance mismatch"
        );
        let peak = g.peak - b.live;
        assert_eq!(
            peak,
            t.peak - s.live,
            "other-thread activity contaminated the phase peak"
        );
        super::Metrics {
            allocations: global.0,
            reallocations: global.1,
            requested_bytes: global.2,
            freed_bytes: global.3,
            live_start_bytes: b.live as usize,
            live_end_bytes: g.live as usize,
            peak_extra_bytes: usize::try_from(peak)
                .expect("phase peak cannot be below initial live"),
        }
    }

    pub(in super::super) fn live(snapshot: Snapshot) -> usize {
        snapshot.global.live as usize
    }

    pub(in super::super) fn restored(before: Snapshot, after: Snapshot) {
        assert_eq!(
            before.tls_failures, after.tls_failures,
            "allocator TLS unavailable within workload lifetime"
        );
        assert_eq!(
            after.global.live, before.global.live,
            "VM owners must return global requested-live to baseline"
        );
        assert_eq!(
            after.thread.live, before.thread.live,
            "VM owners must return thread requested-live to baseline"
        );
        let g = (
            after.global.allocations - before.global.allocations,
            after.global.reallocations - before.global.reallocations,
            after.global.requested - before.global.requested,
            after.global.freed - before.global.freed,
        );
        let t = (
            after.thread.allocations - before.thread.allocations,
            after.thread.reallocations - before.thread.reallocations,
            after.thread.requested - before.thread.requested,
            after.thread.freed - before.thread.freed,
        );
        assert_eq!(
            g, t,
            "other-thread activity contaminated the workload lifetime"
        );
        assert_eq!(
            g.2, g.3,
            "whole-workload allocation requests must balance frees"
        );
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Metrics {
    pub(super) allocations: usize,
    pub(super) reallocations: usize,
    pub(super) requested_bytes: usize,
    pub(super) freed_bytes: usize,
    pub(super) live_start_bytes: usize,
    pub(super) live_end_bytes: usize,
    pub(super) peak_extra_bytes: usize,
}

#[cfg(feature = "vm-memory-count")]
pub(super) use counted::{begin, interval, live, restored, snapshot, Snapshot};

#[cfg(not(feature = "vm-memory-count"))]
#[derive(Clone, Copy)]
pub(super) struct Snapshot;
#[cfg(not(feature = "vm-memory-count"))]
pub(super) fn snapshot() -> Snapshot {
    Snapshot
}
#[cfg(not(feature = "vm-memory-count"))]
pub(super) fn begin() -> Snapshot {
    Snapshot
}
#[cfg(not(feature = "vm-memory-count"))]
pub(super) fn interval(_: Snapshot, _: Snapshot) -> Metrics {
    Metrics::default()
}
#[cfg(not(feature = "vm-memory-count"))]
pub(super) fn live(_: Snapshot) -> usize {
    0
}
#[cfg(not(feature = "vm-memory-count"))]
pub(super) fn restored(_: Snapshot, _: Snapshot) {}
