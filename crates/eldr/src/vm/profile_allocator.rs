//! Requested allocation traffic in one test thread's explicit scope.
//! This module and its System wrapper are absent from product builds.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Traffic {
    pub requested_bytes: u64,
    pub freed_bytes: u64,
    pub alloc_calls: u64,
    pub realloc_calls: u64,
    pub dealloc_calls: u64,
}

thread_local! {
    static TRAFFIC: Cell<Option<Traffic>> = const { Cell::new(None) };
}
static ACTIVE_SCOPES: AtomicUsize = AtomicUsize::new(0);

fn record(requested: usize, freed: usize, alloc: u64, realloc: u64, dealloc: u64) {
    if ACTIVE_SCOPES.load(Ordering::Relaxed) == 0 {
        return;
    }
    // TLS can already be destroyed during thread shutdown. No allocation,
    // assertion, logging or panic is allowed inside the allocator wrapper.
    let _ = TRAFFIC.try_with(|cell| {
        if let Some(mut traffic) = cell.get() {
            traffic.requested_bytes = traffic.requested_bytes.saturating_add(requested as u64);
            traffic.freed_bytes = traffic.freed_bytes.saturating_add(freed as u64);
            traffic.alloc_calls = traffic.alloc_calls.saturating_add(alloc);
            traffic.realloc_calls = traffic.realloc_calls.saturating_add(realloc);
            traffic.dealloc_calls = traffic.dealloc_calls.saturating_add(dealloc);
            cell.set(Some(traffic));
        }
    });
}

struct ScopedSystem;

// SAFETY: all pointers/layouts are forwarded unchanged to System. Accounting
// only touches allocation-free TLS and cannot unwind across this interface.
unsafe impl GlobalAlloc for ScopedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record(layout.size(), 0, 1, 0, 0);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record(layout.size(), 0, 1, 0, 0);
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        if !replacement.is_null() {
            // Count the full requested replacement, not just capacity growth.
            record(new_size, layout.size(), 0, 1, 0);
        }
        replacement
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        record(0, layout.size(), 0, 0, 1);
    }
}

#[global_allocator]
static ALLOCATOR: ScopedSystem = ScopedSystem;

pub(super) struct Scope {
    active: bool,
    // A scope must finish on the thread where it began.
    _thread: PhantomData<Rc<()>>,
}

impl Scope {
    pub fn start() -> Self {
        TRAFFIC.with(|cell| {
            assert!(cell.get().is_none(), "allocation scopes cannot nest");
            cell.set(Some(Traffic::default()));
        });
        ACTIVE_SCOPES.fetch_add(1, Ordering::Relaxed);
        Self {
            active: true,
            _thread: PhantomData,
        }
    }

    pub fn finish(mut self) -> Traffic {
        let traffic = TRAFFIC.with(|cell| cell.replace(None).unwrap());
        ACTIVE_SCOPES.fetch_sub(1, Ordering::Relaxed);
        self.active = false;
        traffic
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.active {
            TRAFFIC.with(|cell| cell.set(None));
            ACTIVE_SCOPES.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[test]
fn scope_counts_requested_traffic_and_frees_of_existing_blocks() {
    use std::alloc::{alloc, alloc_zeroed, dealloc, handle_alloc_error, realloc};
    use std::hint::black_box;
    let existing_layout = Layout::from_size_align(16, 8).unwrap();
    let first_layout = Layout::from_size_align(32, 8).unwrap();
    let second_layout = Layout::from_size_align(64, 8).unwrap();
    let replacement_layout = Layout::from_size_align(96, 8).unwrap();
    unsafe {
        let existing = black_box(alloc(existing_layout));
        assert!(!existing.is_null());
        let scope = Scope::start();
        let first = black_box(alloc(first_layout));
        if first.is_null() {
            handle_alloc_error(first_layout);
        }
        let second = black_box(alloc_zeroed(second_layout));
        if second.is_null() {
            handle_alloc_error(second_layout);
        }
        let replacement = black_box(realloc(second, second_layout, 96));
        if replacement.is_null() {
            handle_alloc_error(replacement_layout);
        }
        dealloc(first, first_layout);
        dealloc(replacement, replacement_layout);
        dealloc(existing, existing_layout);
        let traffic = scope.finish();
        assert_eq!(traffic.requested_bytes, 192);
        assert_eq!(traffic.freed_bytes, 208);
        assert_eq!(
            (
                traffic.alloc_calls,
                traffic.realloc_calls,
                traffic.dealloc_calls
            ),
            (2, 1, 3)
        );
    }
}
