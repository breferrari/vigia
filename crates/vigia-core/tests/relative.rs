//! I1, for the invocation the tool is named after.
//!
//! > `vigia` is Portuguese: a watchman, and a porthole. Also the verb: `vigia .`
//! > reads as "watch this."
//!
//! `vigia .` drew its first frame and then ignored every filesystem event for
//! the rest of the session, silently, because the worktree root spelled `"."`
//! matched none of the paths events carry. Every one of the ten tests in
//! `watch.rs` passed throughout, because [`Scratch::worktree`] discovers by an
//! absolute `PathBuf` and so the relative case had never once been run.
//!
//! **This file holds exactly one test, and that is the point.** Reproducing the
//! defect needs the worktree root to be `"."` exactly: Rust's `Components`
//! normalises a `.` away everywhere except the start of a path, so even
//! `<absolute>/.` strips correctly and looks healthy. Getting a root of `"."`
//! means changing the process working directory, which is global and would race
//! every other test sharing the binary. A test target is a process, so this one
//! owns its directory by being alone in it.
//!
//! The rule itself is unit tested beside the code, in `watch.rs`'s `roots_of`.
//! This is the end-to-end half: the rule can be right while the wiring is
//! wrong, and only a real watch over a real repository says otherwise.

mod support;

use std::sync::mpsc;
use std::time::Duration;

use support::Scratch;
use vigia_core::{Tick, WatchOptions, Watcher, Worktree};

/// How long a real change is allowed to take to travel through the OS.
///
/// The same generous bound `watch.rs` uses, and generous for the same reason:
/// this is a concession to CI, and tightening it would buy flakiness rather
/// than rigour.
const SETTLE: Duration = Duration::from_secs(10);

/// How long the writer waits, so the write lands after the watch is armed.
const DELAY: Duration = Duration::from_millis(400);

/// Block on `next_tick`, with another thread stopping the watcher after
/// `timeout`, so `None` means "still waiting" rather than "hung".
///
/// A local copy rather than a shared helper, which is how the rest of these
/// suites handle a mechanism: `support` is shared only for [`support::slack`],
/// because that is one policy rather than one convenience.
fn tick_within(watcher: &mut Watcher<'_>, timeout: Duration) -> Option<Tick> {
    let stop = watcher.stopper();
    let (done, finished) = mpsc::channel::<()>();
    let guard = std::thread::spawn(move || {
        // Wakes early once the tick arrives, so a passing test does not pay the
        // whole timeout.
        if finished.recv_timeout(timeout).is_err() {
            stop.stop();
        }
    });

    let tick = watcher.next_tick();
    let _ = done.send(());
    let _ = guard.join();
    tick
}

#[test]
fn a_worktree_discovered_by_a_relative_path_still_ticks() {
    let scratch = Scratch::new("relative-root");
    scratch.write("src/lib.rs", "fn before() {}\n");
    scratch.commit_all("baseline");

    let original = std::env::current_dir().expect("read the working directory");
    std::env::set_current_dir(scratch.path_of(".")).expect("enter the scratch repository");

    // Everything that can fail happens in here, so the working directory is put
    // back before any assertion runs. On Windows a directory that is some
    // process's current one cannot be removed, so a panic inside this block
    // would leak the whole fixture past `Scratch`'s own `Drop`.
    let outcome = std::panic::catch_unwind(|| {
        let worktree = Worktree::discover(".").expect("discover by a relative path");
        let workdir = worktree.workdir().to_path_buf();
        let mut watcher = worktree.watch(WatchOptions::default()).expect("watch");

        let target = std::env::current_dir()
            .expect("read the working directory")
            .join("src/lib.rs");
        std::thread::spawn(move || {
            std::thread::sleep(DELAY);
            std::fs::write(&target, "fn after() { let value = 1; }\n").expect("write");
        });

        let tick = tick_within(&mut watcher, SETTLE);
        (workdir, tick, watcher.delivered(), watcher.stats().filtered)
    });

    std::env::set_current_dir(original).expect("leave the scratch repository");

    let (workdir, tick, delivered, filtered) = outcome.expect("the probe panicked");

    // Non-vacuity, and it is the whole reason this file exists: if `gix` ever
    // starts absolutising what it was given, the root is no longer `"."` and
    // this test would pass while covering nothing.
    assert_eq!(
        workdir,
        std::path::PathBuf::from("."),
        "the worktree root is not spelled `.`, so this test no longer reaches \
         the case it was written for"
    );
    assert!(
        delivered > 0,
        "the OS reported no events at all, so nothing was measured"
    );

    assert!(
        tick.is_some(),
        "a write inside a worktree discovered as `.` produced no tick: \
         {delivered} events were delivered and {filtered} of them were \
         discarded as outside the worktree"
    );
}
