//! A panic caught inside one picture's work, and the flag that tells the
//! panic hook to leave it to the catch.
//!
//! The release build unwinds so a malformed image cannot close the pane, but
//! the hook runs before any unwinding: it has to know, on the panicking
//! thread, that a catch is waiting. A thread name cannot say that, because the
//! same thread runs work outside the catch too.

use std::cell::Cell;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU8, Ordering};

thread_local! {
    static CATCHING: Cell<bool> = const { Cell::new(false) };
    /// Panics the hook has seen inside the current catch.
    static SEEN: Cell<u32> = const { Cell::new(0) };
}

/// `work`, or `None` if it panicked. The panic hook leaves a panic in here to
/// this catch.
pub fn caught<T>(work: impl FnOnce() -> T) -> Option<T> {
    let outer = CATCHING.with(|catching| catching.replace(true));
    let seen = SEEN.with(|seen| seen.replace(0));
    let result = std::panic::catch_unwind(AssertUnwindSafe(work));
    CATCHING.with(|catching| catching.set(outer));
    SEEN.with(|cell| cell.set(seen));
    result.ok()
}

/// Whether the panic the hook is running for will reach a [`caught`] on this
/// thread. A second one inside the same catch is raised while the first
/// unwinds, and the runtime aborts on it, so it is not.
pub fn catching() -> bool {
    CATCHING.with(Cell::get) && SEEN.with(|seen| seen.replace(seen.get() + 1)) == 0
}

/// Where a test has asked the next picture to panic, or nothing.
static ARMED: AtomicU8 = AtomicU8::new(0);

/// Which part of reading a picture [`panic_next`] makes panic.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Reading an image's header, on the frame's thread.
    Header = 1,
    /// Decoding it, on the picture worker.
    Decode = 2,
    /// Encoding a panel for the terminal.
    Encode = 3,
    /// The picture worker, outside any catch.
    Worker = 4,
}

/// Make the next picture panic at `stage`, as a malformed file would. A test
/// seam: in release it costs one relaxed load at each stage it guards.
#[doc(hidden)]
pub fn panic_next(stage: Stage) {
    ARMED.store(stage as u8, Ordering::Relaxed);
}

/// Panic here if a test armed `stage`.
#[doc(hidden)]
pub fn armed(stage: Stage) {
    if ARMED.load(Ordering::Relaxed) == stage as u8 {
        ARMED.store(0, Ordering::Relaxed);
        panic!("a test asked {stage:?} to panic");
    }
}
