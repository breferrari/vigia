//! I1: redraw is event-driven, never a fixed timer.
//!
//! The invariant has two halves and they need different kinds of proof. That
//! nothing happens while idle is proved by timing: `next_tick` must not return
//! before something actually changes. That the right things happen is proved by
//! filtering: ignored churn and git object writes must not reach the display,
//! while a real edit and an index write must.
//!
//! Every filter test also asserts that the OS delivered events at all. Without
//! that, a test claiming "ignored writes produced no tick" would pass just as
//! happily on a machine where the watcher was silently broken.

mod support;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use support::Scratch;
use vigia_core::{Tick, WatchOptions, Watcher};

/// How long to insist that nothing happens.
const IDLE: Duration = Duration::from_millis(600);

/// How long a real change is allowed to take to travel through the OS.
///
/// Generous on purpose. This bound existing at all is a concession to CI, and
/// making it tight would buy flakiness rather than rigour.
const SETTLE: Duration = Duration::from_secs(10);

/// How long the writer thread waits before touching anything.
const DELAY: Duration = Duration::from_millis(400);

/// Block on `next_tick`, but have another thread stop the watcher after
/// `timeout`.
///
/// `None` therefore means the watcher was still waiting when time ran out,
/// which is exactly the idle assertion. The watcher is spent after a `None`
/// and must not be reused.
///
/// The watcher itself never crosses a thread boundary here; only a [`Stop`]
/// handle does.
fn tick_within(watcher: &mut Watcher<'_>, timeout: Duration) -> Option<Tick> {
    let stop = watcher.stopper();
    let (done, finished) = mpsc::channel::<()>();
    let guard = std::thread::spawn(move || {
        // Wakes early once the tick has arrived, so a passing test does not
        // pay the whole timeout.
        if finished.recv_timeout(timeout).is_err() {
            stop.stop();
        }
    });

    let tick = watcher.next_tick();
    let _ = done.send(());
    let _ = guard.join();
    tick
}

fn committed_scratch(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write("a.txt", "x\n");
    scratch.commit_all("initial");
    scratch
}

#[test]
fn the_watcher_sleeps_until_something_actually_changes() {
    let scratch = committed_scratch("watch-blocking");
    let worktree = scratch.worktree();
    let mut watcher = worktree.watch(WatchOptions::default()).expect("watch");

    let target = scratch.path_of("a.txt");
    let writer = std::thread::spawn(move || {
        std::thread::sleep(DELAY);
        std::fs::write(&target, "y\n").expect("write from the other thread");
    });

    let started = Instant::now();
    let tick = tick_within(&mut watcher, SETTLE);
    let waited = started.elapsed();
    writer.join().expect("writer thread");

    assert!(tick.is_some(), "a real change produced no tick");
    // The load-bearing assertion. A timer anywhere in the wait would return
    // here before the writer thread had done anything.
    assert!(
        waited >= DELAY,
        "next_tick returned after {waited:?}, before anything had changed"
    );
}

#[test]
fn an_untouched_worktree_delivers_no_events_at_all() {
    let scratch = committed_scratch("watch-idle");
    let worktree = scratch.worktree();
    let watcher = worktree.watch(WatchOptions::default()).expect("watch");

    std::thread::sleep(IDLE);

    assert_eq!(
        watcher.delivered(),
        0,
        "the OS delivered events for a worktree nobody touched"
    );
}

#[test]
fn a_burst_of_writes_becomes_one_tick() {
    let scratch = committed_scratch("watch-burst");
    let worktree = scratch.worktree();
    // A wide quiet window so the whole burst lands inside it even on a loaded
    // machine. The coalescing mechanism is under test, not the default timing.
    let options = WatchOptions {
        quiet: Duration::from_millis(400),
        max_delay: Duration::from_secs(5),
    };
    let mut watcher = worktree.watch(options).expect("watch");

    for i in 0..30 {
        scratch.write(&format!("burst/file_{i}.txt"), "content\n");
    }

    let tick = tick_within(&mut watcher, SETTLE).expect("a burst must produce a tick");
    assert!(
        tick.events >= 2,
        "expected several events folded into one tick, got {}",
        tick.events
    );
    assert!(
        tick_within(&mut watcher, IDLE).is_none(),
        "one burst produced a second tick"
    );
}

#[test]
fn a_continuous_writer_still_gets_a_tick_within_max_delay() {
    let scratch = committed_scratch("watch-max-delay");
    let worktree = scratch.worktree();
    // Quiet is never reached while the writer runs, so only max_delay can end
    // the burst. Without it the display would starve for as long as the writes
    // continued.
    let options = WatchOptions {
        quiet: Duration::from_millis(500),
        max_delay: Duration::from_millis(300),
    };
    let mut watcher = worktree.watch(options).expect("watch");

    let root = scratch.path_of("churn");
    std::fs::create_dir_all(&root).expect("create churn dir");
    let (stop_writing, keep_writing) = mpsc::channel::<()>();
    let writer = std::thread::spawn(move || {
        let mut i = 0u32;
        while keep_writing.try_recv().is_err() {
            let _ = std::fs::write(root.join(format!("f{i}.txt")), "x\n");
            i += 1;
            std::thread::sleep(Duration::from_millis(20));
        }
    });

    let tick = tick_within(&mut watcher, SETTLE).expect("continuous writes produced no tick");
    let _ = stop_writing.send(());
    writer.join().expect("writer thread");

    assert!(
        tick.coalesced_for < Duration::from_millis(500),
        "the burst was held for {:?}, past its max_delay",
        tick.coalesced_for
    );
}

#[test]
fn writes_to_ignored_paths_never_produce_a_tick() {
    let scratch = Scratch::new("watch-ignored");
    scratch.write(".gitignore", "target/\n");
    scratch.write("a.txt", "x\n");
    scratch.commit_all("initial");

    let worktree = scratch.worktree();
    let mut watcher = worktree.watch(WatchOptions::default()).expect("watch");

    // The shape of a cargo build: many writes, none of them the user's.
    for i in 0..40 {
        scratch.write(&format!("target/debug/artifact_{i}.o"), "not source\n");
    }

    assert!(
        tick_within(&mut watcher, IDLE).is_none(),
        "ignored churn woke the monitor"
    );
    assert!(
        watcher.delivered() > 0,
        "this test proved nothing: the OS never reported the ignored writes"
    );
    assert!(
        watcher.stats().filtered > 0,
        "events arrived but none were recorded as filtered"
    );
}

#[test]
fn git_object_churn_never_produces_a_tick() {
    let scratch = committed_scratch("watch-git-noise");
    let worktree = scratch.worktree();
    let mut watcher = worktree.watch(WatchOptions::default()).expect("watch");

    // What a commit does in bulk. None of it changes a rendered pixel.
    for i in 0..20 {
        scratch.write(&format!(".git/objects/ab/{i:038x}"), "z");
    }
    scratch.write(".git/COMMIT_EDITMSG", "subject\n");

    assert!(
        tick_within(&mut watcher, IDLE).is_none(),
        "git internal writes woke the monitor"
    );
    assert!(
        watcher.delivered() > 0,
        "this test proved nothing: the OS never reported the git writes"
    );
}

#[test]
fn staging_a_change_produces_a_tick() {
    let scratch = committed_scratch("watch-index");
    scratch.write("a.txt", "y\n");

    let worktree = scratch.worktree();
    let mut watcher = worktree.watch(WatchOptions::default()).expect("watch");

    // Staging moves the left-hand side of the diff, so the monitor has to
    // notice even though no worktree file was touched.
    scratch.git(&["add", "a.txt"]);

    assert!(
        tick_within(&mut watcher, SETTLE).is_some(),
        "an index write produced no tick"
    );
}

#[test]
fn a_stopper_unblocks_a_waiting_watcher() {
    let scratch = committed_scratch("watch-stop");
    let worktree = scratch.worktree();
    let mut watcher = worktree.watch(WatchOptions::default()).expect("watch");

    let stop = watcher.stopper();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        stop.stop();
    });

    assert_eq!(
        watcher.next_tick(),
        None,
        "stop did not unblock a waiting next_tick"
    );
    stopper.join().expect("stopper thread");
}
