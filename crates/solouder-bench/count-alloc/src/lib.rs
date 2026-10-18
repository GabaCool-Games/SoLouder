//! A global allocator that counts the allocations, frees and reallocations
//! made on the audio thread while it renders, and nowhere else: the game and
//! stream threads allocate freely at the same time. Used by
//! `solouder-bench`'s soak test; not published.
//!
//! `GlobalAlloc` is an unsafe trait, so this package is the one place in the
//! workspace that allows unsafe code, in its one `impl`. Every method
//! forwards to `System` with the caller's arguments unchanged.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// [`System`], counting what the rendering audio thread does.
pub struct Counting;

static AUDIO_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    // Const-initialized and without a destructor, so reading it never
    // allocates (which would recurse into the allocator).
    static RENDERING: Cell<bool> = const { Cell::new(false) };
}

fn note() {
    if RENDERING.try_with(Cell::get).unwrap_or(false) {
        AUDIO_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: every method forwards its arguments unchanged to `System`, which
// upholds the `GlobalAlloc` contract; `note` only reads a thread-local flag
// and bumps an atomic, and never allocates.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller's contract for `alloc` is passed on as is.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: as for `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note();
        // SAFETY: `ptr` was allocated by `System` (through this allocator)
        // with `layout`, as the caller guarantees.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: as for `dealloc`, and `new_size` is the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Runs `render` counted as the audio thread's render.
pub fn rendering<R>(render: impl FnOnce() -> R) -> R {
    RENDERING.set(true);
    let result = render();
    RENDERING.set(false);
    result
}

/// Allocations, frees and reallocations made inside [`rendering`] so far.
pub fn audio_allocations() -> usize {
    AUDIO_ALLOCATIONS.load(Ordering::Relaxed)
}
