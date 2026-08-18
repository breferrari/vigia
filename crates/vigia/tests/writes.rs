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
//! * **The terminal takeover.** `Session::enter` needs a tty. §7 names that
//!   carve-out for the **soak** specifically, in the bullet listing what the soak
//!   leaves out, and `first_paint.rs`'s own docblock is where it is named for
//!   `budgets.rs` and `first_paint.rs`.
//!
//!   Two drafts of this bullet got that wrong in the same place. The first said
//!   *"I8 is what covers that path"*, which is false: I8 is about the terminal
//!   being **restored** on every exit the process controls, a different claim from
//!   writing nothing. The second credited §7 with naming the carve-out for all
//!   three files, which §7 does not say. So: nothing covers writes on that path.
//!   `crates/vigia/src/terminal.rs` makes no filesystem call today and no test
//!   says so, and a panic-hook crash dump is exactly the kind of good-faith
//!   addition I8's own panic requirement invites.
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
use vigia::{Action, App, Glyphs, PaintStats, Theme, body_layout, render};
use vigia_core::{Frame, Highlighter, History, WARM_FILES, WatchOptions, Worktree};

use support::{Scratch, made_link};

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
/// all.
///
/// **Mutation-verified, and only after the fixture gained a link.** This paragraph
/// argued the point for a while over a fixture with no symlink under it, which made
/// it unfalsifiable: with nothing to resolve, `metadata` and `symlink_metadata`
/// agree everywhere. With one present, swapping to `metadata` fails the walk
/// outright on that entry: `Os { code: 123, kind: InvalidFilename }`, which is
/// Windows refusing the forward-slash target and is #15's reading reproduced by a
/// gate rather than asserted in a comment.
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
    /// NTFS, and the limit is the one §6 already documents, stated at its real
    /// width rather than the narrower version this comment carried first: **any
    /// change that leaves a length identical and lands inside one modification-time
    /// granule** is invisible here. A create and delete is one instance and needs
    /// the parent's entry list to settle back; a same-length rewrite of a file that
    /// already existed is another and touches no directory at all. Unreachable by
    /// hand on NTFS, ext4 or APFS, where granules are sub-millisecond. Real on the
    /// 1s and 2s granules of HFS+, FAT and exFAT that §6's own table names, and
    /// there is no fix for it in a `stat`-shaped instrument: content hashing is the
    /// only thing that closes it, and that is a different gate at a different cost.
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
    let meta = std::fs::symlink_metadata(path)
        .unwrap_or_else(|e| panic!("stamp the fixture entry {}: {e}", path.display()));
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
    let mut rig = Rig {
        app: App::new(),
        frame: worktree.frame(),
        highlighter: Highlighter::new(),
        history: History::new(),
        theme: Theme::default(),
        buf: Buffer::empty(area()),
        frames: 0,
        body_rows: 0,
        painted: PaintStats::default(),
        leanest_frame: u64::MAX,
    };
    rig.frame.advance().expect("advance");

    // **A real watch, armed for the length of the run.** `Frame::advance` called by
    // hand is not what "while it runs" means: `vigia::run` arms a `notify` watch and
    // so does the soak. `Watcher::new` reads `.git/index` and the gitignore rules
    // the filter needs, and then arms `notify` over the whole worktree, which is
    // both a second reader of the repository's own files and the one stage that
    // registers with the OS. Calling `advance` directly walks past all of it.
    //
    // What this does *not* reproduce is the second `Worktree` §6 describes: `run`
    // opens one inside the watch thread, because `gix::Repository` is `Send` and not
    // `Sync` so a borrow cannot cross the boundary. Here the watcher borrows the
    // worktree this function already has, on this thread. One repository discovery
    // rather than two, and it is a difference worth naming rather than implying.
    //
    // Held to the end of the function rather than dropped here, so arming and
    // teardown are both inside the compared window. Nothing calls `next_tick`: it
    // blocks on an untimed `recv` by design, which is how I1 gets its zero-wakeup
    // idle, and a gate that waited on an event no write is going to produce would
    // hang rather than fail.
    let watcher = worktree
        .watch(WatchOptions::default())
        .expect("arm a real watch");

    // **The height comes from the frame before each action, not from the first
    // one.** `Action::Page` is measured in the diff's rows, and the diff's rows are
    // what is left after `Footer::plan`, which depends on the follow state that
    // `Action::ToggleFollow` moves three actions in. Carrying the first paint's
    // number through all six would page by a step the screen stopped having, which
    // is a defect a gate about writes would never fail on and a reader would copy.
    let mut height = rig.paint();
    for action in [
        Action::Scroll(12),
        Action::Page(1),
        Action::Bottom,
        Action::Top,
        Action::ToggleFollow,
        Action::ScrollList(3),
    ] {
        rig.app
            .apply(action, &mut rig.frame, height)
            .expect("apply a reader action");
        height = rig.paint();
    }

    rig.frame.advance().expect("advance a second time");
    rig.paint();

    // **Joined rather than detached, which is the opposite of what `run` does and
    // is right here.** `run` drops the handle because nothing needs the result;
    // this gate needs the thread to have *finished* before the second stamp, or a
    // write it made would land after the comparison and the gate would pass by
    // racing. It is the one stage that both spawns a thread and touches the
    // filesystem, so it is the last place a write could hide.
    let warmer = rig.highlighter.warm_ahead(
        worktree.workdir().to_path_buf(),
        rig.frame
            .files()
            .iter()
            .take(WARM_FILES)
            .map(|change| change.path.clone())
            .collect(),
    );
    // **The watch is read here and torn down by going out of scope**, which is what
    // puts its teardown inside the window: `drive` returns before the caller takes
    // the second reading. An earlier version called `watcher.stopper().stop()`
    // first and said in a comment that this ordered the teardown. It did not.
    // `Stop::stop` is `let _ = tx.send(Message::Stop)`, nothing here ever calls
    // `next_tick`, so the message was never read and the call had no observable
    // effect at all; what actually stops the OS watch is dropping `_backend`, which
    // that field's own doc in `watch.rs` says. It was not the product's shutdown
    // path either, which was the other half of the excuse: `vigia::run` never calls
    // `stopper` or `stop`. A line doing nothing under a comment claiming it does
    // something is worse than no line, so it is gone.
    Driven {
        frames: rig.frames,
        body_rows: rig.body_rows,
        content_rows: rig.painted.rows,
        // **Collapsed to zero when no frame ran, so the guard rests on its own
        // evidence.** The running minimum starts at `u64::MAX` and a `drive` that
        // painted nothing would carry that into a `> 0` check and pass it.
        //
        // Belt and braces rather than a fix, and the round-4 audit is what
        // established which: `frames` and the minimum are written in the same two
        // lines of `Rig::paint`, so `frames == 8` cannot hold while the minimum is
        // still `u64::MAX`. The hole was structurally unreachable, not merely
        // guarded by the order the assertions happen to sit in. This line stays
        // because it costs nothing and removes a step the next reader would
        // otherwise have to redo, but it was not a live defect and saying so is the
        // difference between a record and a story.
        leanest_frame: if rig.frames == 0 {
            0
        } else {
            rig.leanest_frame
        },
        warmed: warmer.join().expect("the warmer finished"),
        events: watcher.delivered(),
    }
}

/// What the drive actually did, for the caller to check before it checks the
/// filesystem.
///
/// **The gate's own vacuity hole, and it needed closing.** "Nothing was written"
/// is satisfied perfectly by a `drive` that does nothing at all, so every
/// assertion in this file would survive gutting the function it is about. That is
/// the shape §7 records twice over, and `first_paint.rs` answers it the same way:
/// it asserts the body is full **before** it asserts the clock. These numbers are
/// that assertion, moved to the caller so they sit beside the comparison they
/// qualify rather than inside the code they describe.
///
/// Not all of them are asserted on, and the split is deliberate rather than
/// untidy: `frames`, `leanest_frame` and `warmed` are the guards, and `body_rows`,
/// `content_rows` and `events` are there so a failure says which of several very
/// different things went wrong. The number of each is left out of this sentence on
/// purpose, because it has been wrong twice: it said three, then four, and the
/// count moved again when a redundant assertion came out.
///
/// **What a healthy run reports**, reference machine, so the next reader can tell a
/// marginal guard from one with room: `frames=8 body_rows=15 content_rows=98
/// leanest_frame=11 warmed=3 events=0`. The two worth reading together are the last
/// two guards: `leanest_frame` clears its bound by **11**, not by one, which is what
/// makes a per-frame minimum affordable rather than a flake waiting for a slower
/// machine; and `warmed=3` is `WARM_PER_GRAMMAR` exactly, which is the symlink being
/// skipped before the budget is touched, as this struct's `warmed` doc predicts.
struct Driven {
    /// Frames painted. Eight: a first paint, six actions and the second tick.
    frames: usize,
    /// The tallest body any of them drew, so a blank pane cannot pass as a run.
    body_rows: usize,
    /// Rows of **content**, summed across the whole run.
    ///
    /// A sum and not a maximum, which is worth saying because the field beside it is
    /// a maximum and this doc said "the tallest of them" until round 4 of the audit
    /// caught it. The two statistics are deliberately different: a total is the right
    /// shape for "was anything ever drawn", and [`Self::leanest_frame`] below is what
    /// carries "was something drawn *every* time". Mixing them up in a failure
    /// message is the trap, so the message names which is which.
    ///
    /// **`body_rows` alone is weaker than it reads, and this is what the round-2
    /// audit caught.** `View::collect` pushes a `Row::File` heading for every
    /// changed file before it reaches a single hunk, so `body_rows > 0` is
    /// guaranteed by the fixture having files in it and would stay green against a
    /// build whose line-drawing was deleted outright. That is the *exact* mutation
    /// `first_paint.rs` names when it says a frame "reached further down the file
    /// list and drew more headings instead", and it is why that gate asserts the
    /// body is **full** rather than non-empty.
    ///
    /// So this counts content specifically, and it takes the count from
    /// **`PaintStats.rows`**, which the renderer returns and which is documented as
    /// exactly this: rows of file content drawn, with headings, hunk headers and
    /// notes excluded. `budgets.rs` and `paint.rs` already treat it as the
    /// authoritative figure.
    ///
    /// **Reusing it rather than filtering `view.rows` is not only tidier, it is the
    /// stronger instrument, and `PaintStats`'s own doc says why**: a counter over
    /// the *output* is true by construction, where that struct counts where the walk
    /// happens. The first version of this field re-derived the same statistic by
    /// matching `Row::Line` over the collected rows, which measured what `collect`
    /// planned rather than what `render` drew.
    ///
    /// **Mutated to prove the difference rather than argued.** With `View::collect`
    /// stopped from pushing any content line, the run draws **15 rows over 8
    /// frames** and `body_rows > 0` is satisfied by every one of them; this counter
    /// reports zero and fails. The two are not degrees of one, and that is the
    /// number that says so.
    content_rows: u64,
    /// Content rows in the **leanest** frame, which is the statistic that bites.
    ///
    /// **A maximum was the wrong choice and it is the one §7 warns about twice.**
    /// Taking the largest content count across the eight frames means one frame
    /// carries the whole guard: a regression that blanks the pane on a single action
    /// leaves the first paint's content intact and the gate green. That is
    /// "a budget measured at one position is measured at its cheapest one" applied
    /// to a counter instead of a clock, and it is the same defect shape §7 records
    /// against `scroll.rs`, where a gate named for a blank pane asserted a number
    /// only the blank pane could satisfy.
    ///
    /// A minimum says *every* frame drew content. It is affordable here because the
    /// fixture is controlled: eight files of forty lines, fully rewritten, in a
    /// 24-row pane, so every position the six actions reach has content under it.
    /// Run ten times consecutively before being trusted, since a guard that flakes
    /// is worse than the weak one it replaced.
    ///
    /// **And the difference is measured, not argued.** With `take_file` drawing
    /// content for the first changed file only, so that the frame after
    /// `Action::Bottom` lands on a file that draws headings alone, the run reports
    /// **8 frames, 15 rows, 5 content rows, leanest 0**. A total or a maximum is
    /// satisfied by that 5 and passes; this fires. That is the whole case for the
    /// statistic, and the first mutation aimed at it **survived** and had to be
    /// re-aimed: gating on `skip` changed nothing, because `skip` is an offset within
    /// the file being drawn and is zero on every frame here. A survivor that proves
    /// the mutation was wrong is not a gate that is weak.
    leanest_frame: u64,
    /// Files the warmer compiled, which is the one stage that spawns a thread.
    ///
    /// **Asserted as "more than none" and not as a count, which the symlink is the
    /// reason to say out loud.** `gix`'s status sorts `link_to_mod_0.rs` ahead of
    /// `src/`, so the link is offered to the warmer first. On Windows its
    /// forward-slash target fails `canonicalize` and is skipped before
    /// `WARM_PER_GRAMMAR` is touched at all; on Linux and macOS it resolves and
    /// consumes one of the three Rust slots, so real `.rs` files warm twice rather
    /// than three times. Harmless to this gate and invisible to `> 0`, and the note
    /// is here because tightening this to `>= 3` would then fail on two platforms
    /// out of three for a reason nothing on the line would explain.
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
/// `Rig` and not `Shell`, which is what it was called first: `crates/vigia/src`
/// already has a private `Shell`, with different fields and a session in it, and
/// two types one word apart in a repository this size is a reader's problem even
/// when it is not the compiler's.
///
/// A struct rather than a closure taking `app`, `frame` and `highlighter` as three
/// `&mut` parameters, which is what this was. The closure form is what the borrow
/// checker forces *for a closure*: capturing them would hold the borrows across
/// the `App::apply` and `Frame::advance` calls that sit between paints. Rust
/// allows disjoint borrows of a struct's own fields, so the struct removes the
/// constraint instead of satisfying it, and eight call sites stop repeating the
/// same three reborrows in the same order. Found by `/simplify`, and it is what
/// makes [`Driven`]'s counters affordable: fields rather than two more arguments.
struct Rig<'w> {
    app: App,
    frame: Frame<'w>,
    highlighter: Highlighter,
    history: History,
    theme: Theme,
    buf: Buffer,
    frames: usize,
    body_rows: usize,
    painted: PaintStats,
    leanest_frame: u64,
}

impl Rig<'_> {
    /// One whole frame: chrome, layout, collect, paint. Returns the diff's rows,
    /// which is what an `Action::Page` step is measured in.
    ///
    /// The chrome and the layout are rebuilt every frame rather than hoisted, and
    /// that is required rather than tidy: `Footer::plan` depends on the follow
    /// state and the file count, and `Action::ToggleFollow` moves the first of
    /// those mid-sequence. `budgets.rs` hoists its own because nothing in its loop
    /// changes either term.
    fn paint(&mut self) -> usize {
        let chrome = self.app.chrome("fixture", None, None, None, None, None);
        let body = body_layout(area(), &chrome, self.frame.files().len());
        let view = self
            .app
            .view(&mut self.frame, &mut self.highlighter, &self.history, body)
            .expect("collect a view");
        let painted = render(
            &mut self.buf,
            area(),
            &view,
            &self.theme,
            Glyphs::default(),
            &chrome,
        );
        self.frames += 1;
        self.body_rows = self.body_rows.max(view.rows.len());
        self.painted += painted;
        self.leanest_frame = self.leanest_frame.min(painted.rows);
        body.diff
    }
}

#[test]
fn the_monitor_writes_nothing_while_it_runs() {
    let scratch = Scratch::large_diff("writes-nothing", FILES, LINES);
    let root = scratch.root().to_path_buf();

    // **A symlink, because [`Stamp`] argues about links at length and no fixture
    // here had one.** §7's rule is that an axis named as unspanned is a prediction,
    // and this file was making the prediction in a doc comment: the reason given for
    // `symlink_metadata` over `metadata` could not fail, because nothing under the
    // root was a link. With one present the choice becomes load bearing rather than
    // argued, and mutation says so: swapped to `metadata`, the walk panics on this
    // entry, which is #15's Windows reading exactly.
    //
    // `made_link` and not `checkout_link`, which is the stronger fixture and would
    // destroy this one: it composes `committed_link`, which runs `commit_all`, and
    // committing here would commit the rewritten files and leave the fixture with no
    // diff in it at all. The target is spelled with a forward slash deliberately,
    // since the OS is handed it verbatim, and a forward-slash target is the form
    // Windows refuses to resolve. `made_link` prints its own note and returns false
    // where the platform will not make one, which is this repo's skip-with-a-reason
    // shape rather than a silent pass.
    //
    // **The axis cannot go missing from the whole matrix**, which is the question a
    // conditional assertion invites. Windows is the only tier-1 target that can
    // refuse a link, and `ci.yml` runs `cargo test --workspace` on ubuntu and macos
    // too, where an unprivileged symlink always succeeds. So the skip narrows
    // coverage on at most one leg of three, by construction rather than by hope,
    // which is why nothing here watches for it.
    let linked = made_link(&scratch, "src/mod_0.rs", "link_to_mod_0.rs");

    let before = snapshot(&root);
    if linked {
        assert!(
            before.contains_key(Path::new("link_to_mod_0.rs")),
            "the walk did not stamp the symlink it was given, so the link is not in \
             the window this gate compares"
        );
    }
    assert!(
        before.len() > FILES,
        "the fixture stamped {} entries, which is no more than its own file count, \
         so the walk is not seeing the tree",
        before.len()
    );

    let driven = drive(&root);

    // **Before the filesystem is compared, not after.** "Nothing was written" is
    // satisfied perfectly by a run that did nothing, so these are what stop the gate
    // below passing against a `drive` somebody gutted. Same order `first_paint.rs`
    // uses: assert the frame was real, then assert the property.
    assert_eq!(
        driven.frames, 8,
        "the drive painted {} frames rather than the first paint, six actions and \
         the second tick, so the window this gate compares is not the one it \
         describes",
        driven.frames
    );
    // **A heading is pushed per changed file before any hunk is reached**, so a body
    // with rows in it proves only that the fixture had files. Content is what a
    // build with its line-drawing deleted stops producing, which is the mutation
    // `first_paint.rs` names in its own comment.
    //
    // There is no separate `body_rows > 0` assertion above this one, and there was
    // until round 3 of the audit pointed out that it cannot fail on its own. The
    // argument needs one step more than it first looks, because the two fields are
    // different statistics: `body_rows` is the **largest** row count any frame drew
    // and `leanest_frame` is the **smallest** content count any frame drew. If the
    // largest is zero then every frame drew nothing, so every frame drew no content,
    // so the smallest is zero and this fires. An assertion whose failure mode is
    // unreachable is one the next reader has to reason about for nothing. The field
    // stays, because the message below is worth more when it can say "rows were
    // drawn and none of them were content" than when it can only say "no content".
    assert!(
        driven.leanest_frame > 0,
        "the leanest of {} frames drew no content at all, against a tallest body of \
         {} rows in any one frame and {} content rows summed across the run, so at \
         least one position drew nothing but headings and the tree this gate found \
         clean was not really read there",
        driven.frames,
        driven.body_rows,
        driven.content_rows
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
    // The third direction, and it had no case here until round 3 of the audit said
    // so. `difference` reports removals, and a comparator with one of its three arms
    // undriven is two thirds tested: a `before` key that vanishes is how a deleted
    // file, a rename's old side, and a temp file cleaned up after itself all look.
    scratch.remove("src/mod_1.rs");

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
    assert!(
        moved
            .iter()
            .any(|line| line.contains("mod_1.rs") && line.contains("was removed")),
        "a removed file was not reported as removed, so the gate above cannot see a \
         deletion: {moved:?}"
    );
}
