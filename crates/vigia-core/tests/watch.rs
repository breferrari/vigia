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

use support::{Scratch, budget};
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

/// Gap between two writes whose *order* is what is under test.
///
/// Long enough that the OS reports them in the order they happened, short
/// enough to sit far inside [`ORDERING_QUIET`] so they still coalesce.
const ORDERING_GAP: Duration = Duration::from_millis(50);

/// Quiet window for the ordering test.
///
/// Twenty times [`ORDERING_GAP`]. Generous because a burst that splits in two
/// does not merely loosen that test, it inverts it: the first half's tick would
/// name the *first* write, which is precisely the wrong answer it exists to
/// catch. Cheap to buy the margin, expensive to debug without it.
const ORDERING_QUIET: Duration = Duration::from_secs(1);

/// The longest a burst held open by a continuous writer may last.
///
/// The `max_delay` under test is 300ms, and this is the deadline plus room for
/// one event's processing to straddle it. See the assertion that uses it for why
/// the engine cannot make the overshoot smaller than a single syscall.
const MAX_DELAY_BOUND: Duration = Duration::from_millis(500);

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
fn an_idle_worktree_produces_no_tick_and_accepts_nothing() {
    let scratch = committed_scratch("watch-idle");
    let worktree = scratch.worktree();
    let mut watcher = worktree.watch(WatchOptions::default()).expect("watch");

    assert!(
        tick_within(&mut watcher, IDLE).is_none(),
        "a tick arrived although nothing changed"
    );

    // Deliberately not asserting that the OS delivered nothing. inotify and
    // FSEvents both report reads and attribute touches, so a tree nobody wrote
    // to is not a tree the kernel is silent about, and an earlier version of
    // this test failed on Linux and macOS for exactly that reason while the
    // engine was behaving correctly.
    //
    // I1 is a claim about work done, so the portable form is that nothing which
    // arrived was ever accepted as a change.
    let stats = watcher.stats();
    assert_eq!(stats.ticks, 0, "an idle tree produced a tick: {stats:?}");
    assert_eq!(
        stats.wakeups,
        stats.filtered + 1,
        "an idle tree accepted an event; every wakeup but the stop that ended \
         the wait should have been filtered: {stats:?}"
    );
}

#[test]
fn a_burst_of_writes_becomes_one_tick() {
    let scratch = Scratch::new("watch-burst");
    scratch.write("a.txt", "x\n");
    // The burst directory has to exist, and be watched, before the burst lands
    // in it. A recursive watch does not cover a directory that did not exist
    // when it was armed: the backend has to notice the new directory and add a
    // watch of its own, and on Linux anything written in the gap between those
    // two is never reported. Creating the directory here rather than with the
    // first write is what makes this test about coalescing rather than about
    // inotify's directory race. Observed on CI as `got 1`, where the tick had
    // folded the directory-creation event and nothing else.
    //
    // I1 does not care about the lost events: every tick triggers a full status
    // walk, so the directory event alone would still have found all thirty
    // files. It is this test's counter that cared.
    scratch.write("burst/.keep", "\n");
    scratch.commit_all("initial");
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

/// I5's input: B2 says follow the write that landed **last** in the batch.
///
/// Both orders are exercised, and that is the whole design of the test. With
/// one order, "the last write" and "any write in the burst" give the same
/// answer, so a implementation that returned the *first* path, or an arbitrary
/// one, would pass. Only the pair separates them.
#[test]
fn a_tick_names_the_file_whose_write_landed_last() {
    let scratch = Scratch::new("watch-newest");
    scratch.write("a.txt", "x\n");
    scratch.write("b.txt", "x\n");
    scratch.commit_all("initial");
    let worktree = scratch.worktree();
    // Both files sit at the worktree root, which exists when the watch is
    // armed. `a_burst_of_writes_becomes_one_tick` explains why that matters:
    // a recursive watch does not cover a directory created after it.
    //
    // A wide quiet window because the burst folding is a precondition here
    // rather than the subject. If the two writes fell into separate ticks the
    // assertion below would be about a batch of one.
    let options = WatchOptions {
        quiet: ORDERING_QUIET,
        max_delay: Duration::from_secs(5),
    };
    let mut watcher = worktree.watch(options).expect("watch");

    for (first, last) in [("a.txt", "b.txt"), ("b.txt", "a.txt")] {
        scratch.write(first, "1\n");
        // Far enough apart that the OS reports them in the order they
        // happened, and far inside the quiet window so they still coalesce.
        std::thread::sleep(ORDERING_GAP);
        scratch.write(last, "2\n");

        let tick = tick_within(&mut watcher, SETTLE).expect("a burst must produce a tick");
        assert_eq!(
            tick.newest(),
            Some(last),
            "wrote {first} then {last}, and the tick named {:?}",
            tick.newest()
        );
    }
}

/// The premise behind the ordering rule, checked against a real filesystem.
///
/// `accept_paths`'s unit tests prove that the *last* path an event names wins.
/// They cannot prove that the last path is the destination, because they build
/// the event by hand: that is a claim about what `notify` delivers, and it is
/// only documented rather than verified.
///
/// This closes the gap from the other side. However the platform reports a
/// rename, as one event carrying two paths or as two events inside one burst,
/// the tick has to name where the file **is** rather than where it was. A
/// backend that reported `[to, from]` would fail here and nowhere else.
#[test]
fn a_rename_is_followed_to_where_the_file_now_is() {
    let scratch = Scratch::new("watch-rename");
    scratch.write("before.txt", "x\n");
    scratch.commit_all("initial");
    let worktree = scratch.worktree();
    let options = WatchOptions {
        quiet: ORDERING_QUIET,
        max_delay: Duration::from_secs(5),
    };
    let mut watcher = worktree.watch(options).expect("watch");

    std::fs::rename(scratch.path_of("before.txt"), scratch.path_of("after.txt"))
        .expect("rename the fixture file");

    let tick = tick_within(&mut watcher, SETTLE).expect("a rename must produce a tick");
    assert_eq!(
        tick.newest(),
        Some("after.txt"),
        "a rename named {:?}, so the view moves to a path that no longer exists",
        tick.newest()
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

    // Two assertions in one, and it is worth separating what each catches. That a
    // tick arrived at all is the invariant: `quiet` is 500ms and is reset by every
    // accepted event, so while the writer runs it can never be satisfied, and only
    // `max_delay` can end the burst. Drop `max_delay` from the engine and the
    // `expect` above fires.
    //
    // The bound below adds "and at roughly the configured value", which catches a
    // deadline honoured at the wrong length. It takes slack because it is an
    // absolute wall-clock bound on a shared runner, which `SPEC.md` §7 already
    // calls a weak instrument, and because the engine cannot make it tight: the
    // burst loop re-checks its deadline once per event, so the overshoot is
    // whatever one `accept` costs, and one `accept` is a `stat` plus a gitignore
    // probe. On a Windows CI runner scanning files created milliseconds earlier
    // that single syscall has been measured at 244ms past the deadline, and on a
    // post-merge run of `main` at **473ms past** (773.68ms held against a 300ms
    // `max_delay`), which is not something the loop can preempt. A developer
    // machine still runs this at zero slack and measures 300.1ms — an overshoot
    // of 0.1ms — so the slack buys nothing locally and everything on a runner.
    //
    // That 773.68ms went red because the `test` step supplied no
    // `VIGIA_BUDGET_SLACK`, so this `budget()` call was resolving to 1. The step
    // sets it now. Do NOT respond to a future red here by raising the constant:
    // it is the local bound too, and at zero slack it is currently tight to
    // within a millisecond.
    assert!(
        tick.coalesced_for < budget(MAX_DELAY_BOUND),
        "the burst was held for {:?}, past the {:?} its max_delay allows",
        tick.coalesced_for,
        budget(MAX_DELAY_BOUND)
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

/// A stop unblocks with `None` even when a burst is already open.
///
/// The burst loop used to break out and fall through to its `Some(Tick)`, so a
/// stop meant "one more tick, then stop" whenever anything was mid-arrival.
/// `tick_within` builds its timeout on a stop, so a timeout could report a tick
/// that had not arrived in time.
///
/// The quiet window is widened well past the default 16ms on purpose: the stop
/// has to land *inside* the burst, and against the default it lands after the
/// burst has already closed on its own and proves nothing.
#[test]
fn a_stop_that_lands_mid_burst_still_returns_none() {
    const QUIET: Duration = Duration::from_millis(500);

    let scratch = committed_scratch("watch-stop-midburst");
    let worktree = scratch.worktree();
    let mut watcher = worktree
        .watch(WatchOptions {
            quiet: QUIET,
            max_delay: Duration::from_secs(5),
        })
        .expect("watch");

    let stop = watcher.stopper();
    // Opens a burst the quiet window will hold open long enough to stop inside.
    scratch.write("a.txt", "y\n");
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        stop.stop();
    });

    let started = std::time::Instant::now();
    let got = watcher.next_tick();
    let waited = started.elapsed();
    stopper.join().expect("stopper thread");

    // Proves the stop is what ended the wait rather than the quiet window:
    // closing on quiet would take QUIET, and this returns in about 60ms.
    assert!(
        waited < QUIET,
        "the burst closed on its own after {waited:?}, so this never tested a stop"
    );
    assert_eq!(
        got, None,
        "a stop landed while a burst was open and returned a tick instead of ending the wait"
    );
}
