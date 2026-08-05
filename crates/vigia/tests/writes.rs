//! `SPEC.md` §11.1: the monitor writes nothing.
//!
//! The rule is older than the line that states it, and older than this file's
//! first attempt to cite it: both quotations below are §10's, and an earlier
//! draft of this docblock attributed them to §6.
//!
//! §10's settle-margin bullet refuses an active probe of the filesystem's
//! timestamp granularity because *"a read-only monitor will not do"* that, and
//! rests the whole two-second constant on *"a monitor never writes, so it only
//! ever sees the gaps its user's tools happened to leave"*. §6 carries the same
//! dependency at rule level rather than in prose: the margin is a table lookup
//! because measuring would need the write. Both are load
//! bearing and neither had a gate, so the property they rest on was true by
//! nobody's decision: `vigia` happens to open no file for writing, and a cache, a
//! lock file or a crash dump added in good faith would have made two recorded
//! arguments quietly wrong.
//!
//! B7 is what made that unaffordable. The ruling against an observation log is
//! *"the monitor does not write"*, so a ruling with no gate under it would be the
//! same shape §7 keeps finding: settled-looking and proving nothing.
//!
//! **What this holds.** Every entry under the worktree root, `.git` included and
//! the root itself included, across a first paint, six reader actions, a second
//! tick, a real armed `notify` watch and the grammar warmer.
//!
//! **What it does not, named rather than left to be discovered.** Three things,
//! and the first was written here as covered when it is not:
//!
//! * **The terminal takeover.** `Session::enter` needs a tty, which is the
//!   carve-out §7 already names for `budgets.rs`, `soak.rs` and `first_paint.rs`.
//!   An earlier draft of this docblock said *"I8 is what covers that path"*, and
//!   that is false: I8 is about the terminal being **restored** on every exit the
//!   process controls, which is a different claim from writing nothing. Nothing
//!   covers writes there. `crates/vigia/src/terminal.rs` makes no filesystem call
//!   today and no test says so, and a panic-hook crash dump is exactly the kind of
//!   good-faith addition I8's own panic requirement invites.
//! * **The event path.** The watch is armed for the length of the run, so the
//!   watcher's own construction, which reads `.git/index` and the gitignore rules,
//!   and its teardown are both inside the window. No event is *delivered*, because
//!   nothing writes during the window, which is the property under test. So the
//!   coalescer's accept path is driven by nothing here; `soak.rs` is where a real
//!   event stream runs.
//! * **Core paths the shell does not take.** Every filesystem call this gate can
//!   reach lives in `vigia-core`, and it reaches them *through the shell*, which is
//!   the process §11.1's rule is about. A consumer using `Frame`, `Worktree` or
//!   `Highlighter` as a library has no write-nothing gate of its own. That is a gap
//!   in `vigia-core`'s suite rather than a flaw in this one, and it is stated
//!   because a reader of B7 could otherwise assume the whole crate is covered.
//!
//! **Length and modification time, not content hashes, and deliberately no access
//! time.** A read is not a write, which is the same reason
//! `vigia_core::watch` drops `EventKind::Access` before it reaches the coalescer:
//! a monitor that redrew because something read a file would be absurd, and a
//! gate that failed for the same reason would be too.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{Action, App, Theme, body_layout, render};
use vigia_core::{Frame, Highlighter, History, WARM_FILES, WatchOptions, Worktree};

use support::Scratch;

/// Small on purpose. This gate counts filesystem entries rather than
/// milliseconds, so the hundred-file fixture the budget gates share would buy it
/// nothing and cost the suite a second.
const FILES: usize = 8;
const LINES: usize = 40;

/// An ordinary terminal, the same one the rendering suites use.
fn area() -> Rect {
    Rect::new(0, 0, 80, 24)
}

/// What one filesystem entry looked like.
///
/// `symlink_metadata` rather than `metadata`, so a link is stamped as itself
/// rather than as whatever it points at. That matters here for the reason §7
/// records against [#15](https://github.com/breferrari/vigia/issues/15): on
/// Windows a link whose stored target uses forward slashes does not resolve at
/// all, and a stamp that returned `None` for every link would compare equal to a
/// stamp that returned `None` for every link, which is agreement about nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    /// Directories are stamped too, which is what catches a file created **and
    /// deleted** inside one window: adding or removing a child moves the parent's
    /// own modification time, and the file itself is in neither map.
    ///
    /// Mutation-verified rather than reasoned about. Writing and immediately
    /// removing a file inside the driven window turns the gate red on that file's
    /// **parent** alone, at 4096 bytes both sides, so the modification time is
    /// carrying the whole signal and the length carries none of it. That is the
    /// claim; the size of the time gap is not, and quoting one was a mistake worth
    /// recording, because 3,816,902 of Windows' 100ns intervals was read off as
    /// 3.8ms and is 382ms. It is elapsed test time, not a granularity figure.
    ///
    /// NTFS, and the limit is the one §6 already documents: a create and delete
    /// inside a single timestamp granule would leave both maps agreeing, so this
    /// is a strong catch rather than a proof.
    dir: bool,
    len: u64,
    /// `None` where the platform refuses one, which is a value like any other:
    /// it has to be *stable*, not present.
    modified: Option<SystemTime>,
}

/// Every entry under `root`, keyed by its path relative to it.
///
/// Iterative rather than recursive so a deep tree cannot end this in a stack
/// overflow that reads as a filesystem finding.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Stamp> {
    let mut found = BTreeMap::new();
    // **The root is stamped as well, and it was not until the mutation run said
    // so.** Every directory below it is reached as an entry of its parent, so the
    // one directory nothing stamps is the top one, and a file created and deleted
    // directly under it inside one window would leave both maps agreeing. The
    // first mutation of this gate reported *"1 entries"* for a created file where
    // the parent's own modification time should have made it two, which is what
    // pointed at the hole.
    found.insert(PathBuf::from("."), stamp_of(root));
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir).expect("read a fixture directory");
        for entry in entries {
            let entry = entry.expect("read a fixture directory entry");
            let path = entry.path();
            let stamp = stamp_of(&path);
            // `rela` first so `path` can be *moved* into the queue rather than
            // cloned into it. One allocation a directory, on a tree this gate
            // keeps small by design, so the reason to do it is that the clone had
            // no reason at all.
            let rela = path
                .strip_prefix(root)
                .expect("an entry below the root it was walked from")
                .to_path_buf();
            let dir = stamp.dir;
            found.insert(rela, stamp);
            if dir {
                pending.push(path);
            }
        }
    }
    found
}

/// One entry's stamp, from a **fresh** query rather than a cached one.
///
/// `symlink_metadata` rather than `metadata`, for the reason [`Stamp`] gives: a
/// link is stamped as itself.
///
/// **And a fresh call per entry rather than `DirEntry::metadata`, which is the
/// optimisation this gate cannot take.** `read_dir` has already filled a directory
/// record carrying times and sizes, so `entry.metadata()` answers out of it with no
/// second syscall, which is why a `/simplify` pass proposed it: one syscall saved
/// per entry, four walks a run, and the same non-following semantics. It was tried
/// and it **broke the gate**. Same code, same window, same armed watch, with the
/// only difference being where the metadata came from: the cached form reported
/// `.git\hooks`, `.git\logs\refs` and `.git\objects` as moved, at zero bytes on
/// both sides, with modification times **1.0ms, 1.0ms and 14.0ms** apart, and the
/// fresh form reported the tree clean. Both cannot be right about one unchanged
/// directory, so the cached value disagrees with the queried one, and Windows
/// updates a directory entry's cached times lazily rather than on the write.
///
/// The lesson is not about the syscall. **A before-and-after comparison is exactly
/// the consumer a cached timestamp cannot serve**, because staleness that would be
/// harmless in a single reading becomes a difference between two readings, and it
/// manufactures a failure in the direction that looks like a real finding: three
/// `.git` directories moving during a run that wrote nothing.
fn stamp_of(path: &Path) -> Stamp {
    let meta = std::fs::symlink_metadata(path).expect("stamp a fixture entry");
    Stamp {
        dir: meta.is_dir(),
        len: meta.len(),
        modified: meta.modified().ok(),
    }
}

/// Every path the two stamps disagree about, as lines fit to print.
///
/// Returned rather than asserted on, because both directions of this comparison
/// are a gate below: one asserts it is empty and the other asserts it is not.
fn difference(before: &BTreeMap<PathBuf, Stamp>, after: &BTreeMap<PathBuf, Stamp>) -> Vec<String> {
    let mut moved = Vec::new();
    for (path, was) in before {
        match after.get(path) {
            None => moved.push(format!("{} was removed", path.display())),
            Some(now) if now != was => moved.push(format!(
                "{} moved: {} bytes at {:?} became {} bytes at {:?}",
                path.display(),
                was.len,
                was.modified,
                now.len,
                now.modified
            )),
            Some(_) => {}
        }
    }
    for path in after.keys() {
        if !before.contains_key(path) {
            moved.push(format!("{} was created", path.display()));
        }
    }
    moved
}

/// A first paint, six reader actions, a second tick and the warmer.
///
/// Staged the way `first_paint.rs::cold_start` stages it, which is the way
/// `vigia::run` does minus the tty. The actions are not decoration: `Scroll` and
/// `Page` move the viewport into hunks nothing has parsed, `Bottom` and `Top` are
/// the jumps §10 records as the expensive first entry into a large hunk,
/// `ToggleFollow` re-engages follow, and `ScrollList` moves the pinned list on its
/// own. Each is a path that could plausibly want to remember something across
/// frames, and remembering it on disk is exactly what this gate forbids.
///
/// **The second `advance` is the tick, and it is where the reuse rule runs.** A
/// frame that revalidates instead of recomputing is the one that `stat`s every
/// changed file, and a `stat` is the syscall nearest to a write in this codebase.
/// A gate that painted once would never reach it.
fn drive(root: &Path) -> Driven {
    let worktree = Worktree::discover(root).expect("discover");
    let mut shell = Shell {
        app: App::new(),
        frame: worktree.frame(),
        highlighter: Highlighter::new(),
        history: History::new(),
        theme: Theme::default(),
        buf: Buffer::empty(area()),
        frames: 0,
        body_rows: 0,
    };
    shell.frame.advance().expect("advance");

    // **A real watch, armed for the length of the run.** `Frame::advance` called by
    // hand is not what "while it runs" means: `vigia::run` arms a `notify` watch and
    // so does the soak, and the watcher opens the repository a *second* time to read
    // `.git/index` and the gitignore rules the filter needs. That is a second reader
    // of the same repository, which is the most plausible place for a write nobody
    // intended, and calling `advance` directly walks straight past it.
    //
    // Held to the end of the function rather than dropped here, so arming and
    // teardown are both inside the compared window. Nothing calls `next_tick`: it
    // blocks on an untimed `recv` by design, which is how I1 gets its zero-wakeup
    // idle, and a gate that waited on an event no write is going to produce would
    // hang rather than fail.
    let watcher = worktree
        .watch(WatchOptions::default())
        .expect("arm a real watch");

    let height = shell.paint();
    for action in [
        Action::Scroll(12),
        Action::Page(1),
        Action::Bottom,
        Action::Top,
        Action::ToggleFollow,
        Action::ScrollList(3),
    ] {
        shell
            .app
            .apply(action, &mut shell.frame, height)
            .expect("apply a reader action");
        shell.paint();
    }

    shell.frame.advance().expect("advance a second time");
    shell.paint();

    // **Joined rather than detached, which is the opposite of what `run` does and
    // is right here.** `run` drops the handle because nothing needs the result;
    // this gate needs the thread to have *finished* before the second stamp, or a
    // write it made would land after the comparison and the gate would pass by
    // racing. It is the one stage that both spawns a thread and touches the
    // filesystem, so it is the last place a write could hide.
    let warmer = shell.highlighter.warm_ahead(
        worktree.workdir().to_path_buf(),
        shell
            .frame
            .files()
            .iter()
            .take(WARM_FILES)
            .map(|change| change.path.clone())
            .collect(),
    );
    let driven = Driven {
        frames: shell.frames,
        body_rows: shell.body_rows,
        warmed: warmer.join().expect("the warmer finished"),
        events: watcher.delivered(),
    };
    // Stopped explicitly rather than by dropping the binding, so the teardown is
    // ordered where it can be read instead of falling out of scope order.
    watcher.stopper().stop();
    driven
}

/// What the drive actually did, for the caller to check before it checks the
/// filesystem.
///
/// **The gate's own vacuity hole, and it needed closing.** "Nothing was written"
/// is satisfied perfectly by a `drive` that does nothing at all, so every
/// assertion in this file would survive gutting the function it is about. That is
/// the shape §7 records twice over, and `first_paint.rs` answers it the same way:
/// it asserts the body is full **before** it asserts the clock. These three
/// numbers are that assertion, moved to the caller so they sit beside the
/// comparison they qualify rather than inside the code they describe.
struct Driven {
    /// Frames painted. Eight: a first paint, six actions and the second tick.
    frames: usize,
    /// The tallest body any of them drew, so a blank pane cannot pass as a run.
    body_rows: usize,
    /// Files the warmer compiled, which is the one stage that spawns a thread.
    warmed: usize,
    /// Raw events the armed watch was handed, **reported and not gated.**
    ///
    /// Zero is what a window with no writes in it should produce, and asserting
    /// that would still be wrong: `notify` is a different backend per platform and
    /// `inotify` can be configured to report reads, which this run does plenty of.
    /// The filter drops `EventKind::Access` for exactly that reason, so a count
    /// here would be gating a platform's taste rather than this code. It earns its
    /// place in the failure message below, where "the tree moved and the watch saw
    /// N events" and "the tree moved and the watch saw none" point at different
    /// culprits.
    ///
    /// **It also happens to be the proof that the watch is not decorative**, which
    /// arming one without ever reading from it badly needs. Reference machine: the
    /// clean run reports **0**, and the mutation that writes a file inside the
    /// window reports **5**. So the thing being armed is delivering, and a future
    /// edit that armed nothing would show as a zero the mutation could not move.
    events: u64,
}

/// The shell's parts, held together so one can be borrowed while another paints.
///
/// A struct rather than a closure taking `app`, `frame` and `highlighter` as three
/// `&mut` parameters, which is what this was. The closure form is what the borrow
/// checker forces *for a closure*: capturing them would hold the borrows across
/// the `App::apply` and `Frame::advance` calls that sit between paints. Rust
/// allows disjoint borrows of a struct's own fields, so the struct removes the
/// constraint instead of satisfying it, and eight call sites stop repeating the
/// same three reborrows in the same order. Found by `/simplify`, and it is what
/// makes [`Driven`]'s counters affordable: fields rather than two more arguments.
struct Shell<'w> {
    app: App,
    frame: Frame<'w>,
    highlighter: Highlighter,
    history: History,
    theme: Theme,
    buf: Buffer,
    frames: usize,
    body_rows: usize,
}

impl Shell<'_> {
    /// One whole frame: chrome, layout, collect, paint. Returns the diff's rows,
    /// which is what an `Action::Page` step is measured in.
    ///
    /// The chrome and the layout are rebuilt every frame rather than hoisted, and
    /// that is required rather than tidy: `Footer::plan` depends on the follow
    /// state and the file count, and `Action::ToggleFollow` moves the first of
    /// those mid-sequence. `budgets.rs` hoists its own because nothing in its loop
    /// changes either term.
    fn paint(&mut self) -> usize {
        let chrome = self.app.chrome("fixture", None);
        let body = body_layout(area(), &chrome, self.frame.files().len());
        let view = self
            .app
            .view(&mut self.frame, &mut self.highlighter, &self.history, body)
            .expect("collect a view");
        render(&mut self.buf, area(), &view, &self.theme, &chrome);
        self.frames += 1;
        self.body_rows = self.body_rows.max(view.rows.len());
        body.diff
    }
}

#[test]
fn the_monitor_writes_nothing_while_it_runs() {
    let scratch = Scratch::large_diff("writes-nothing", FILES, LINES);
    let root = scratch.root().to_path_buf();

    let before = snapshot(&root);
    assert!(
        before.len() > FILES,
        "the fixture stamped {} entries, which is fewer than its own files, so \
         the walk is not seeing the tree",
        before.len()
    );

    let driven = drive(&root);

    // **Before the filesystem is compared, not after.** "Nothing was written" is
    // satisfied perfectly by a run that did nothing, so these three are what stop
    // the gate below passing against a `drive` somebody gutted. Same order
    // `first_paint.rs` uses: assert the frame was real, then assert the property.
    assert_eq!(
        driven.frames, 8,
        "the drive painted {} frames rather than the first paint, six actions and \
         the second tick, so the window this gate compares is not the one it \
         describes",
        driven.frames
    );
    assert!(
        driven.body_rows > 0,
        "every frame drew an empty body, so the run this gate calls clean never \
         put a diff on screen"
    );
    assert!(
        driven.warmed > 0,
        "the warmer compiled nothing, so the one stage that spawns a thread was \
         not exercised and this gate is a paint test wearing its name"
    );

    let moved = difference(&before, &snapshot(&root));
    assert!(
        moved.is_empty(),
        "SPEC.md §11.1: the monitor writes nothing, and this run moved {} \
         entries over {} frames, with {} events delivered to the watch:\n{}",
        moved.len(),
        driven.frames,
        driven.events,
        moved.join("\n")
    );
}

#[test]
fn the_snapshot_sees_a_write_that_does_happen() {
    let scratch = Scratch::large_diff("writes-detected", FILES, LINES);
    let root = scratch.root().to_path_buf();

    let before = snapshot(&root);

    // **A different length, deliberately.** Two writes of the same length inside
    // one modification-time granule are indistinguishable by `stat`, which is
    // §6's racily-clean case and is the flake a stamp resting on the clock alone
    // would be. Growing the file takes the comparison off that axis entirely.
    scratch.write("src/mod_0.rs", "one line\n".repeat(LINES * 2));
    scratch.write("untracked.txt", "and a file that was not there before\n");

    let moved = difference(&before, &snapshot(&root));
    assert!(
        moved.iter().any(|line| line.contains("mod_0.rs")),
        "a rewritten file was not reported, so the gate above cannot see a \
         modification: {moved:?}"
    );
    assert!(
        moved.iter().any(|line| line.contains("untracked.txt")),
        "a created file was not reported, so the gate above cannot see a new \
         file: {moved:?}"
    );
}
