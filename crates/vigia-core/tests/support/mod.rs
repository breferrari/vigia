// Each test binary compiles this module separately, so anything only one of
// them uses reads as dead code in the others.
#![allow(dead_code)]

//! A disposable git repository, built by real `git`.
//!
//! Fixtures are created by shelling out rather than by `gix` on purpose. The
//! open question Phase 1 answers is whether `gix` reads a working tree the way
//! git wrote it, and a fixture written by the library under test cannot answer
//! that.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use vigia_core::{CONTEXT, FileChange, Frame, FrameStats, HighlightStats, Worktree};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Idle frames allowed while a fixture's writes settle.
///
/// Generous because a spare frame costs milliseconds, and because the assertion
/// [`settle`] ends with is what gives the number teeth.
const SETTLE_FRAMES: usize = 8;

/// How long to wait for a fixture's own writes to become provably old.
///
/// This has to exceed the engine's own settle margin, which is two seconds
/// because a filesystem's modification-time granularity can be that coarse. It
/// is a real wait and cannot be a spin: the whole point of the margin is that no
/// amount of looking at a file makes its granule close sooner.
///
/// Tests run concurrently, so this costs the suite one wait rather than one per
/// test.
const SETTLE_WAIT: Duration = Duration::from_millis(2_500);

/// Multiplier applied to absolute wall-clock bounds, and to nothing else.
///
/// Defaults to 1, so a developer machine is held to `SPEC.md` exactly. CI raises
/// it because hosted runners are shared and their variance is not a property of
/// this code. Structural gates ignore it entirely.
///
/// Shared rather than declared per test binary, which is the exception to how the
/// rest of these suites handle helpers. It is one policy, named in `SPEC.md` §7 by
/// its environment variable, and two copies of it would be free to drift into
/// disagreeing about what CI is allowed to be slow by.
pub fn slack() -> f64 {
    std::env::var("VIGIA_BUDGET_SLACK")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .filter(|value: &f64| *value >= 1.0)
        .unwrap_or(1.0)
}

/// `base`, loosened by [`slack`].
pub fn budget(base: Duration) -> Duration {
    base.mul_f64(slack())
}

/// How long `work` took, for a stage whose result nothing downstream needs.
///
/// Shared for the reason the policy helpers above are: a timer is the one helper
/// where a divergent copy silently changes what a budget gate measured. `FnOnce`
/// rather than `FnMut`, which subsumes every call site the suites had.
pub fn time(work: impl FnOnce()) -> Duration {
    timed(work).1
}

/// [`time`], for a stage that produces something the next stage needs.
pub fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = work();
    (value, start.elapsed())
}

/// Whether the absolute wall-clock tier should assert.
///
/// A debug build is several times slower than the one the budgets were set
/// against, so asserting there would fail for a reason that is not a regression.
/// Reported rather than silently skipped, and `how` is the exact command that
/// enforces it, because a note telling a reader to re-run without saying how is
/// a note they have to go and work out.
///
/// Here for the reason [`slack`] and [`exclusively_timed`] are: it is one policy
/// and copies drift. **This one had already drifted before it was folded**, in
/// the half that has to be right. Two of the four copies printed
/// `cargo test --release --test budgets`, which is ambiguous because both crates
/// ship a `budgets.rs`, and the four split three to one on whether it was one
/// gate being skipped or several. Taking `how` does not make a wrong hint impossible —
/// it is still hand-typed per call site — but it puts the command beside the
/// gate it reruns instead of inside a helper copied between crates.
pub fn absolute_gates_apply(how: &str) -> bool {
    if cfg!(debug_assertions) {
        eprintln!("note: the absolute budget gates are skipped in a debug build; run `{how}`");
        false
    } else {
        true
    }
}

/// Held for the timed region of an absolute gate, so two never overlap.
///
/// `cargo test` runs a binary's tests on parallel threads, so absolute gates in
/// the same binary measure each other. That is tolerable while they all do the
/// same light per-frame edit and stops being tolerable the moment one of them
/// writes a fixture: a 1.5 MiB bulk rewrite took its two neighbours from passing
/// to **53 and 54ms p99** against a 16ms budget, while their p50 stayed at 6.6
/// and 7.6ms. A p50 that holds while the p99 goes eight times over is
/// contention, not a regression, and no threshold tells the two apart.
///
/// Here rather than in one test binary for the same reason [`slack`] is here:
/// it is one policy, `SPEC.md` §7 already calls an absolute gate on a shared
/// machine a weak instrument, and two copies would be free to drift. Each test
/// binary compiles its own `static`, which is exactly the scope wanted, since
/// `cargo` runs the binaries themselves one at a time.
///
/// Poison is unwrapped through deliberately. A gate that fails while holding
/// this has already reported the real number, and letting the panic cascade into
/// poison failures would bury it under neighbours that are fine.
static TIMED: Mutex<()> = Mutex::new(());

pub fn exclusively_timed() -> MutexGuard<'static, ()> {
    TIMED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What git reports for one path, with binary distinguishable from unchanged.
///
/// See [`Scratch::git_numstat`] for why the third variant exists.
#[derive(Debug, PartialEq, Eq)]
pub enum Numstat {
    /// git reports no difference at all, and printed nothing.
    Unchanged,
    /// git considers the path binary and printed `-` for both counts.
    Binary,
    /// Lines added and removed.
    Lines(u32, u32),
}

/// Make `link` point at `target`, or say why this platform is not checking it.
///
/// Refusing is reported rather than silently turning the gate that follows into
/// a pass, which is the shape `SPEC.md` §7 keeps naming. Windows needs
/// `SeCreateSymbolicLinkPrivilege` or developer mode, so a hosted runner may
/// decline; Linux and macOS never do, so
/// [#15](https://github.com/breferrari/vigia/issues/15)'s gates run on every
/// push whatever this machine does.
///
/// Here rather than in one test binary because two of them need it and the note
/// is the part that must not drift: a skip that stops explaining itself is a
/// silent pass again.
pub fn made_link(scratch: &Scratch, target: &str, link: &str) -> bool {
    if scratch.symlink_file(target, link) {
        return true;
    }
    eprintln!(
        "note: this platform would not create the file symlink {link} -> {target}, \
         so #15's reading is unchecked here; it is checked wherever one can be made"
    );
    false
}

/// The other half of non-vacuity: git has to have stored a symlink.
///
/// Creating a link the platform accepts is not the same as git recording one.
/// With `core.symlinks=false` git writes and stores a plain file holding the
/// target path, and a fixture git itself sees no symlink in cannot fail a test
/// about symlinks however the code behaves.
pub fn git_stored_a_symlink(scratch: &Scratch, link: &str) -> bool {
    let mode = scratch.index_mode(link);
    if mode == "120000" {
        return true;
    }
    // **An empty mode is the caller's bug, not the platform's, so it panics
    // rather than skipping.** `index_mode` shells out to `git ls-files -s`,
    // which reports nothing at all for a path that is not in the index, so
    // asking before `commit_all` returns `""` here. That is indistinguishable
    // from "this checkout does not do symlinks" at the call site, and it would
    // print the note and skip: a green test that ran no assertion, which is the
    // exact failure both these helpers exist to prevent. Six hand-written copies
    // of the link/commit/verify order made that a live risk; `committed_link`
    // now owns the order, and this is what catches the seventh site that does
    // not use it.
    assert!(
        !mode.is_empty(),
        "{link} is not in the index, so this fixture was checked before it was \
         committed and the skip below would have been silent"
    );
    eprintln!(
        "note: git recorded {link} as mode {mode} rather than 120000, so this \
         fixture holds no symlink and #15's reading is unchecked here"
    );
    false
}

/// Link, commit, and confirm git recorded mode `120000`.
///
/// `false` means this platform or this checkout is not holding a symlink and the
/// caller should return; both halves have already printed why.
///
/// **The order is the point.** [`git_stored_a_symlink`] reads the *index*, so it
/// is meaningless before the commit, and a caller that got the order wrong used
/// to skip silently. This is that protocol written once instead of six times.
pub fn committed_link(scratch: &Scratch, target: &str, link: &str) -> bool {
    if !made_link(scratch, target, link) {
        return false;
    }
    scratch.commit_all("initial");
    git_stored_a_symlink(scratch, link)
}

/// Every change, sorted by path so assertions do not depend on walk order.
///
/// Named for the sort rather than for the call, because that is the property
/// tests here rely on and because `budgets.rs` keeps its own unsorted spelling
/// on purpose: it measures the walk, so reordering what the walk reported would
/// be measuring something else.
pub fn changes_sorted(worktree: &Worktree) -> Vec<FileChange> {
    let mut all: Vec<FileChange> = worktree
        .changes()
        .expect("enumerate changes")
        .map(|c| c.expect("change"))
        .collect();
    all.sort_by(|a, b| a.path.cmp(&b.path));
    all
}

/// `count` lines of `line N`, newline terminated.
///
/// The plainest fixture content there is, and deliberately not [`generated`]:
/// these lines are short enough to reason about a hunk header by eye, which is
/// what the fidelity suites do with them.
pub fn numbered_lines(count: usize) -> String {
    (1..=count).map(|i| format!("line {i}\n")).collect()
}

/// Advance one frame and fetch every diff in it.
///
/// Fetching *all* of them is the point. A frame path that lazily diffs nothing
/// satisfies I2a vacuously, so a test asks for everything and counts what that
/// cost.
pub fn materialise(frame: &mut Frame) {
    frame.advance().expect("advance");
    for i in 0..frame.files().len() {
        frame.diff(i).expect("diff");
    }
}

/// Wait for the frame's files to become provably unchanged, then prove they did.
///
/// A file written moments ago cannot be *proved* unchanged, because a filesystem
/// floors the modification time it stamps and the granule may still be open. So
/// this waits out the engine's margin before measuring anything. Fixtures are
/// written immediately before a test runs, so without the wait the first frames
/// legitimately re-read everything. I2a is a claim about the frame after an edit
/// has landed, not the frame racing the write.
///
/// This cannot wait out a broken cache. A frame path that never reuses anything
/// never settles, and the panic below is what it gets.
pub fn settle(frame: &mut Frame) {
    std::thread::sleep(SETTLE_WAIT);
    for _ in 0..SETTLE_FRAMES {
        let before = frame.stats().computed;
        materialise(frame);
        if frame.stats().computed == before {
            return;
        }
    }
    panic!(
        "the frame was still re-reading after {SETTLE_FRAMES} idle frames, \
         so nothing is ever being reused"
    );
}

/// Wait out the margin and fill every **span**, diffing nothing.
///
/// [`settle`]'s sibling, and the difference between them is the whole of
/// [#101](https://github.com/breferrari/vigia/issues/101). `settle` proves the
/// fixture is old by calling [`materialise`], which diffs *every* file, and a
/// frame whose diffs are all cached rebuilds every span from a `FileDiff`
/// already in hand and reads nothing. So a gate that opens with `settle` has
/// deleted the cost of the height walk before timing anything, which is why the
/// walk was re-reading the whole worktree on every tick for two phases with
/// eleven read-bounding gates green over it.
///
/// This reaches the same settled state by the route the product takes: advance,
/// then ask for the height, which measures the files nothing has drawn and
/// leaves them **un-diffed**. That is the state a reader is in one second after
/// launch, and the only one in which "re-measure everything" and "re-measure
/// what changed" produce different numbers.
///
/// **Takes no `rows_of`, because one would be inert.** [`Frame::height`] takes
/// one so that its *return value* can be a row count, and this discards the
/// total: what it wants is the walk's side effect, which visits every changed
/// file whatever the closure says. Threading a caller's real `rows_of` through
/// would read as though the choice mattered here, and would put the shell's
/// ruling about conflicts and binaries inside a helper that never consults it.
///
/// **Asserts nothing about reuse, deliberately.** Whether a second tick
/// re-measures is the thing under test, and a helper that panicked on it would
/// report "no span is ever carried" from inside the setup instead of letting the
/// gate report its own number. Returns what the priming tick measured so the
/// caller can check its own premise.
pub fn settle_spans(frame: &mut Frame) -> u64 {
    std::thread::sleep(SETTLE_WAIT);
    let before = frame.stats().measured;
    frame.advance().expect("advance");
    frame.height(|_, _| 0).expect("height");
    frame.stats().measured - before
}

/// What one frame cost the highlighter, as the difference between two readings.
///
/// The same shape as [`delta`], and for the same reason: I2b is a claim about
/// one frame, and the counters are cumulative so a test can subtract.
pub fn highlight_delta(before: HighlightStats, after: HighlightStats) -> HighlightStats {
    HighlightStats {
        parsed: after.parsed - before.parsed,
        reused: after.reused - before.reused,
        lines: after.lines - before.lines,
        bytes: after.bytes - before.bytes,
        evicted: after.evicted - before.evicted,
    }
}

/// What one frame cost, as the difference between two cumulative readings.
pub fn delta(before: FrameStats, after: FrameStats) -> FrameStats {
    FrameStats {
        computed: after.computed - before.computed,
        reused: after.reused - before.reused,
        measured: after.measured - before.measured,
        bytes: after.bytes - before.bytes,
        probes: after.probes - before.probes,
        evicted: after.evicted - before.evicted,
    }
}

/// A temporary git repository, removed on drop.
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Create an initialised repository with deterministic config.
    pub fn new(name: &str) -> Self {
        Self::in_dir(&std::env::temp_dir(), name)
    }

    /// The same thing, somewhere other than the temp directory.
    ///
    /// Which sounds like a convenience and is a requirement: the soak asserts
    /// that its own temp directory is still **empty** when the run ends, so its
    /// worktree is the one fixture that cannot live in one. Every other test is
    /// happy in `temp_dir`, and [`Scratch::new`] keeps them there.
    pub fn in_dir(parent: &Path, name: &str) -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            "vigia-test-{}-{}-{}",
            std::process::id(),
            unique,
            name
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create scratch dir");

        let scratch = Scratch { path };
        scratch.git(&["init", "-q", "-b", "main"]);
        // Every one of these exists to stop a developer's global config from
        // changing what the test measures. autocrlf in particular is commonly
        // true on Windows and would rewrite line endings under us.
        scratch.git(&["config", "core.autocrlf", "false"]);
        scratch.git(&["config", "core.safecrlf", "false"]);
        scratch.git(&["config", "user.email", "test@example.invalid"]);
        scratch.git(&["config", "user.name", "vigia tests"]);
        scratch.git(&["config", "commit.gpgsign", "false"]);
        scratch
    }

    /// A repository configured the way a Windows checkout is, so that git's
    /// clean filter has something to do.
    ///
    /// **A sibling of [`Scratch::new`] rather than a replacement for it.** Every
    /// other fixture here sets `core.autocrlf false` at creation, which puts the
    /// whole fixture population on one point of git's configuration surface: LF
    /// on both sides, so the filter never converts anything and the code path
    /// that skips it produces byte-identical output to the code path that runs
    /// it. That is why [#65](https://github.com/breferrari/vigia/issues/65) was
    /// invisible to a green suite, and `SPEC.md` §7 now names the shape. The
    /// answer is to **span** the axis, not to move to its other end, so the
    /// default stays where it is and this stands beside it.
    ///
    /// `attributes` is the `.gitattributes` to write, or `None` for the plain
    /// installed default. The two are different fixtures and not degrees of one:
    /// with no attributes a checkout puts CRLF on disk against an LF blob, and
    /// with `* text=auto eol=lf` it puts LF on disk and only an editor writing
    /// CRLF creates the discrepancy.
    pub fn crlf_worktree(name: &str, attributes: Option<&str>) -> Self {
        let scratch = Self::new(name);
        scratch.git(&["config", "core.autocrlf", "true"]);
        if let Some(attributes) = attributes {
            scratch.write(".gitattributes", attributes);
        }
        scratch
    }

    /// Write a file with CRLF terminators, the way an editor on Windows does.
    ///
    /// Takes LF text and converts, so a test states its content once in the
    /// form it is reasoning about. Refuses text that already carries a `\r`,
    /// which would silently produce `\r\r\n` and a fixture testing nothing.
    pub fn write_crlf(&self, rela: &str, contents: &str) {
        assert!(
            !contents.contains('\r'),
            "{rela} already carries a carriage return, so converting would double it"
        );
        self.write(rela, contents.replace('\n', "\r\n"));
    }

    /// A repository whose working tree differs from its index by every line of
    /// every file: `2 * files * lines` changed lines in total.
    ///
    /// Used to size fixtures against the budgets, which are written in lines of
    /// diff rather than in files.
    pub fn large_diff(name: &str, files: usize, lines: usize) -> Self {
        let scratch = Self::new(name);
        scratch.fill_large_diff(files, lines);
        scratch
    }

    /// The body of [`Scratch::large_diff`], for a repository that already
    /// exists because it had to be created somewhere specific.
    ///
    /// Split rather than duplicated: two spellings of one fixture would drift,
    /// and the soak's numbers are only comparable to the budget gates' while
    /// both are measuring the same repository.
    pub fn fill_large_diff(&self, files: usize, lines: usize) {
        self.fill_pairs(
            files,
            |f| format!("src/mod_{f}.rs"),
            |tag| generated(lines, tag),
        );
    }

    /// Write `files` files, commit them, then rewrite every one line for line.
    ///
    /// The shape both fixture families are: a baseline commit and a working tree
    /// differing from it at every line, so a file is exactly one hunk of
    /// `2 * lines` display rows.
    ///
    /// Shared rather than spelled twice because the gates compare the two
    /// families' numbers **against each other**, and that is only like for like
    /// while they differ in the two terms named here and nowhere else.
    /// `fill_large_diff`'s own doc already made that argument for one family;
    /// this is what makes it true of both rather than asserted in prose.
    fn fill_pairs(
        &self,
        files: usize,
        path: impl Fn(usize) -> String,
        content: impl Fn(&str) -> String,
    ) {
        for f in 0..files {
            self.write(&path(f), content("before"));
        }
        self.commit_all("baseline");
        for f in 0..files {
            self.write(&path(f), content("after"));
        }
    }

    /// A repository whose files differ from the index at every `every`th line,
    /// and nowhere else.
    ///
    /// The shape I2b needs and [`Scratch::large_diff`] cannot give it. Rewriting
    /// every line produces exactly **one** hunk per file, and "only the hunk that
    /// changed is re-parsed" cannot be measured against a file with one hunk in
    /// it: reusing nothing and reusing everything look identical.
    ///
    /// Two lines closer together than twice [`CONTEXT`] share a hunk, so `every`
    /// has to clear that with room rather than sit on the boundary. The first
    /// edit is at index `every` rather than 0, so every hunk has leading context
    /// and they are all the same shape, which is what lets a test name a hunk by
    /// its ordinal and know how tall it is.
    ///
    /// Each edited line is written as [`generated`] would have written it, so two
    /// fixtures of **different lengths hold identical content wherever they
    /// overlap**. That is what makes the two-fixture form of the I2b gate a
    /// like-for-like comparison rather than two unrelated numbers.
    pub fn sparse_edits(name: &str, files: usize, lines: usize, every: usize) -> Self {
        assert!(
            every > CONTEXT as usize * 2 + 1,
            "edits {every} lines apart share a hunk, so the fixture has fewer \
             hunks than it looks like"
        );
        assert!(
            lines > every,
            "a {lines}-line file edited every {every} lines has no hunks at all"
        );

        let scratch = Self::new(name);
        for f in 0..files {
            scratch.write(&format!("src/mod_{f}.rs"), generated(lines, "before"));
        }
        scratch.commit_all("baseline");
        for f in 0..files {
            scratch.rewrite(&format!("src/mod_{f}.rs"), |lines| {
                let mut at = every;
                while at < lines.len() {
                    lines[at] = generated_line(at, "after");
                    at += every;
                }
            });
        }
        scratch
    }

    /// A repository of long lines mixing Japanese, emoji and Latin.
    ///
    /// The shape [#45](https://github.com/breferrari/vigia/issues/45) was
    /// reported against, and the one every other fixture here is blind to:
    /// [`generated`] writes 34-column ASCII lines, which fit inside any pane a
    /// gate draws, so *"a drawn row costs its whole line"* and *"a drawn row
    /// costs the pane"* are the same number over all of them.
    ///
    /// See [`wide_line`] for the shape and the arithmetic behind it. Built
    /// through the same [`Scratch::fill_pairs`] as [`Scratch::large_diff`]
    /// rather than a second spelling of it, so "the two fixtures differ in one
    /// term" is a property of the code instead of a claim in a comment.
    ///
    /// `ext` is a parameter for one reason: [`WIDE_UNPARSED_EXT`] is an
    /// extension `syntect` has no grammar for, so the same bytes can be drawn
    /// with the parse and without it. That is what lets the parse be attributed
    /// by **subtraction** rather than by argument, which is what
    /// [#45](https://github.com/breferrari/vigia/issues/45) asks for: three
    /// suspects, each with a number, rather than three ranked by how plausible
    /// they sound.
    pub fn wide_lines_as(name: &str, files: usize, lines: usize, ext: &str) -> Self {
        let scratch = Self::new(name);
        scratch.fill_pairs(
            files,
            |f| format!("docs/note_{f}.{ext}"),
            |tag| wide_generated(lines, tag),
        );
        scratch
    }

    /// Rewrite every file of a [`Scratch::large_diff`] fixture, line for line.
    ///
    /// A formatter, a branch switch and a multi-file agent edit all produce this
    /// shape, and it is the event the settle margin is argued about: every file
    /// changes at once, so for the length of the margin **no** file can be
    /// proved unchanged and every one a caller asks for is recomputed.
    ///
    /// `round` varies the content, and what it buys is the *highlighter* rather
    /// than the frame path. Measured both ways over two consecutive rewrites: a
    /// rewrite with identical bytes still moves the modification time, so the
    /// fingerprint differs and the frame path recomputes regardless
    /// (`computed = 1` either way). What identical bytes leave alone is the
    /// **diff**, so the visible hunk hashes the same and the parse is reused
    /// (`parsed = 0, reused = 1, lines = 0` against `parsed = 1, lines = 20`).
    /// A caller rewriting on a loop with a fixed `round` would therefore measure
    /// frames with the syntax parser idle, which is the cheap half of a frame
    /// and not the event.
    ///
    /// The caller passes `files` and `lines` rather than this remembering them,
    /// because a fixture built with different numbers would be silently
    /// truncated or padded here instead of failing.
    pub fn rewrite_all(&self, files: usize, lines: usize, round: usize) {
        for f in 0..files {
            self.write(
                &format!("src/mod_{f}.rs"),
                generated(lines, &format!("bulk{round}")),
            );
        }
    }

    /// The worktree root.
    pub fn root(&self) -> &Path {
        &self.path
    }

    /// Absolute path of something inside the repository.
    pub fn path_of(&self, rela: &str) -> PathBuf {
        self.path.join(rela)
    }

    /// Run a git command, asserting it succeeded.
    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"));
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Write a file, creating parent directories.
    pub fn write(&self, rela: &str, contents: impl AsRef<[u8]>) {
        let full = self.path.join(rela);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&full, contents).expect("write fixture file");
    }

    /// Delete a file.
    pub fn remove(&self, rela: &str) {
        std::fs::remove_file(self.path.join(rela)).expect("remove fixture file");
    }

    /// Point `link` at `target`, both worktree-relative, replacing any link
    /// already there. `false` means this platform would not make one.
    ///
    /// Windows needs `SeCreateSymbolicLinkPrivilege` or developer mode, so a
    /// hosted runner may refuse. Every caller reports the refusal rather than
    /// letting it turn a gate into a silent pass, which is the shape `SPEC.md`
    /// §7 keeps naming.
    ///
    /// `target` is taken as a `&str` and passed through unchanged rather than
    /// being joined onto the root, because the *text* is what git stores and
    /// what the diff compares. Joining would make every fixture absolute and
    /// test a target no repository holds.
    ///
    /// **Deliberately not folded with `warm.rs::link_dir`.** They look like one
    /// policy in two copies and are not: Windows has separate `symlink_file` and
    /// `symlink_dir` calls with different semantics, and `link_dir` points
    /// *outside* the scratch on purpose, where this is worktree-relative by
    /// construction.
    pub fn symlink_file(&self, target: &str, link: &str) -> bool {
        let full = self.path.join(link);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        // Repointing is remove-then-create on every platform here, which is
        // also what `ln -sfn` does. Ignored rather than asserted: the common
        // case is that there is nothing to remove.
        let _ = std::fs::remove_file(&full);

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, &full).is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(target, &full).is_ok()
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = target;
            false
        }
    }

    /// The index mode git recorded for one path, as the six digits it prints.
    ///
    /// The non-vacuity half of every symlink gate here. A platform can create a
    /// link that git then stores as a regular file (`core.symlinks=false` is the
    /// documented way), and a fixture where git itself sees no symlink cannot
    /// fail a test about symlinks however the code behaves.
    pub fn index_mode(&self, rela: &str) -> String {
        let out = self.git(&["ls-files", "-s", "--", rela]);
        out.split_whitespace().next().unwrap_or_default().to_owned()
    }

    /// Replace one line of a file, leaving every other byte alone.
    ///
    /// The edit I2a is written against: one line, in one file, of many.
    pub fn edit_line(&self, rela: &str, line: usize, text: &str) {
        self.rewrite(rela, |lines| {
            lines[line] = text.to_owned();
        });
    }

    /// Change one character of a line, keeping the file's length identical.
    ///
    /// A same-length edit is the half of an in-place write that a `stat` cannot
    /// see, so it is what the frame path's staleness rule has to survive.
    pub fn scribble_line(&self, rela: &str, line: usize, marker: char) {
        assert!(
            marker.is_ascii(),
            "a same-length edit needs an ASCII marker"
        );
        self.rewrite(rela, |lines| {
            let target = &mut lines[line];
            let last = target
                .char_indices()
                .next_back()
                .expect("fixture lines are never empty")
                .0;
            target.replace_range(last.., &marker.to_string());
        });
    }

    /// Read a file as lines, hand them to `edit`, and write them back.
    ///
    /// Fixture files always end in a newline, so rejoining restores the file
    /// byte for byte apart from what `edit` changed.
    fn rewrite(&self, rela: &str, edit: impl FnOnce(&mut Vec<String>)) {
        let full = self.path.join(rela);
        let content = std::fs::read_to_string(&full).expect("read fixture file");
        assert!(
            content.ends_with('\n'),
            "{rela} does not end in a newline, so rewriting it would change its shape"
        );
        let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();
        edit(&mut lines);
        let mut joined = lines.join("\n");
        joined.push('\n');
        std::fs::write(&full, joined).expect("write fixture file");
    }

    /// Write `content` into the object database and return its blob id.
    ///
    /// The file it is hashed from is removed again, so the working tree ends up
    /// exactly as it started. Useful for producing a blob that is deliberately
    /// unlike anything the fixture contains: every generated file holds the
    /// same bytes, so their committed blobs are all the *same* object, and
    /// reaching for one of those to stand in for "some other content" quietly
    /// tests nothing.
    pub fn hash_object(&self, content: &str) -> String {
        let rela = ".vigia-hash-object";
        self.write(rela, content);
        let id = self
            .git(&["hash-object", "-w", "--", rela])
            .trim()
            .to_owned();
        self.remove(rela);
        id
    }

    /// Stage everything and commit.
    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// Open the working tree through the crate under test.
    pub fn worktree(&self) -> Worktree {
        Worktree::discover(&self.path).expect("discover scratch repository")
    }

    /// Put a file on disk the way `git checkout` would, rather than the way the
    /// test wrote it.
    ///
    /// The difference is the whole fixture for the CRLF cases: under
    /// `core.autocrlf=true` a checkout writes CRLF even though the test's own
    /// source text is LF, so going through git is what produces the state a
    /// Windows reader actually has. Writing those bytes by hand would test the
    /// same content for a reason that could stop being true.
    pub fn checkout(&self, rela: &str) {
        std::fs::remove_file(self.path_of(rela)).expect("remove before checkout");
        self.git(&["checkout", "--", rela]);
    }

    /// What `git diff --numstat` says about one path.
    ///
    /// Beside [`Scratch::git_hunk_headers`] because it is the same job: ask git
    /// the question the engine just answered, and compare. `SPEC.md` §10 records
    /// git as the oracle, so the accessors that reach it live together.
    ///
    /// Three answers rather than an `Option<(u32, u32)>`, and the third is why.
    /// git prints `-` for both counts on a path it considers binary, so a parser
    /// returning `None` for "no output" collapses **binary** and **unchanged**
    /// onto one value. A test asserting `None` to mean "git sees no change"
    /// would then keep passing if its fixture ever became binary, which is
    /// `SPEC.md` §7's "gate that passes for the wrong reason" exactly.
    pub fn git_numstat(&self, rela: &str) -> Numstat {
        let out = self.git(&["diff", "--numstat", "--", rela]);
        let Some(line) = out.lines().next() else {
            return Numstat::Unchanged;
        };
        let mut fields = line.split('\t');
        let added = fields.next().expect("numstat added field");
        let removed = fields.next().expect("numstat removed field");
        if added == "-" || removed == "-" {
            return Numstat::Binary;
        }
        Numstat::Lines(
            added.parse().expect("numstat added count"),
            removed.parse().expect("numstat removed count"),
        )
    }

    /// Hunk headers as real git reports them, for fidelity comparison.
    ///
    /// Returns `(old_start, old_lines, new_start, new_lines)` per hunk.
    pub fn git_hunk_headers(&self, rela: &str) -> Vec<(u32, u32, u32, u32)> {
        let out = self.git(&["diff", "-U3", "--", rela]);
        out.lines()
            .filter(|line| line.starts_with("@@"))
            .map(parse_hunk_header)
            .collect()
    }
}

/// Plausible source lines, distinct on both sides so every line differs.
///
/// Public because the soak has to put a file **back** to the bytes the index
/// holds, which is how a path leaves the diff and is evicted. Reaching for
/// `git checkout` instead would put `git` inside the measured window, and the
/// temp-file gate is exact only while nothing but the libraries under test can
/// reach the temp directory.
pub fn generated(lines: usize, tag: &str) -> String {
    joined(lines, tag, generated_line)
}

/// `lines` lines from `line`, each newline terminated.
///
/// The termination is why this is shared rather than written once per generator.
/// It is a fixture **policy** and not a detail: [`Scratch::rewrite`] asserts on
/// it, so a generator that quietly stopped terminating would break `edit_line`
/// and `scribble_line` for whichever fixture family forgot.
fn joined(lines: usize, tag: &str, line: impl Fn(usize, &str) -> String) -> String {
    (0..lines)
        .map(|at| {
            let mut one = line(at, tag);
            one.push('\n');
            one
        })
        .collect()
}

/// The one line [`generated`] writes at `at`, with no line ending.
///
/// Split out so [`Scratch::sparse_edits`] can write a single line the same way,
/// which is what keeps two fixtures of different lengths byte-identical wherever
/// they overlap. Written twice, the two would eventually disagree and the
/// two-fixture gates would compare unlike things while still passing.
fn generated_line(at: usize, tag: &str) -> String {
    let n = at + 1;
    format!("fn {tag}_{n}() {{ let value = {}; }}", n * 7)
}

/// Extension the wide fixture uses by default.
///
/// Markdown because that is what was reported: a Japanese README. It also puts
/// the grammar under a case nothing else here reaches, since every other fixture
/// is Rust.
pub const WIDE_EXT: &str = "md";

/// An extension `syntect` has no grammar for.
///
/// `syntax_for` returns `None` and the file draws exactly as it did before there
/// was highlighting at all (`SPEC.md` §11.1), which is a zero-parse baseline over
/// byte-identical content. Asserted rather than assumed: a gate using it checks
/// that the run highlighted **no** lines, so the day `syntect` learns this
/// extension the baseline fails instead of quietly becoming a second measurement
/// of the same thing.
pub const WIDE_UNPARSED_EXT: &str = "vigia";

/// The repeating unit of a [`wide_line`], in source characters.
///
/// `日本語のテキストが続きます。` is 14, `🎉` is 1, and
/// ` and then some latin words follow. ` is 35.
pub const WIDE_UNIT_CHARS: usize = 50;

/// The same unit in terminal columns: 28 + 2 + 35.
///
/// The gap between this and [`WIDE_UNIT_CHARS`] is the whole point of the
/// fixture. A pane bounds **columns**, a walk over the line costs **characters**,
/// and over ASCII the two are equal so nothing can tell a bound from its absence.
pub const WIDE_UNIT_COLUMNS: usize = 65;

/// Units per line.
pub const WIDE_UNITS: usize = 8;

/// Long lines of mixed Japanese, emoji and Latin, one per line, newline
/// terminated.
///
/// Public for the same reason [`generated`] is: a caller has to be able to put a
/// file **back** to what the index holds without reaching for `git`.
pub fn wide_generated(lines: usize, tag: &str) -> String {
    joined(lines, tag, wide_line)
}

/// The one line [`wide_generated`] writes at `at`, with no line ending.
///
/// **The shape, stated rather than left to be counted**, because
/// [#45](https://github.com/breferrari/vigia/issues/45)'s first acceptance
/// criterion asks for it and because every number the gates report is relative to
/// it. An `at` under 1000 gives a prefix of 10 or 11 characters, so a line is
///
/// | | characters | columns | bytes |
/// |---|---|---|---|
/// | prefix `NNN. after: ` | ~11 | ~11 | ~11 |
/// | 8 x unit | 400 | 520 | 648 |
/// | **line** | **~411** | **~531** | **~659** |
///
/// Against the 74-column text area of an 80-column pane that is **7.2x more line
/// than pane**, which is the ratio a bound has to remove and the reason an ASCII
/// fixture cannot see one.
///
/// The three scripts are not decoration. Latin is one column per character and
/// takes the renderer's ASCII path; the Japanese is two columns and does not; and
/// the emoji is two columns from a single non-ASCII character, which is the case
/// where a bound written in characters rather than columns lands a glyph half
/// over the pane's edge.
fn wide_line(at: usize, tag: &str) -> String {
    let n = at + 1;
    let mut line = format!("{n}. {tag}: ");
    for _ in 0..WIDE_UNITS {
        line.push_str("日本語のテキストが続きます。🎉 and then some latin words follow. ");
    }
    line
}

/// Parse `@@ -a,b +c,d @@`, where `,b` is omitted when the count is 1.
fn parse_hunk_header(line: &str) -> (u32, u32, u32, u32) {
    let body = line
        .trim_start_matches('@')
        .split("@@")
        .next()
        .expect("hunk header body")
        .trim();
    let mut parts = body.split_whitespace();
    let old = parts.next().expect("old range");
    let new = parts.next().expect("new range");

    let range = |s: &str| -> (u32, u32) {
        let s = s.trim_start_matches(['-', '+']);
        match s.split_once(',') {
            Some((start, count)) => (start.parse().unwrap(), count.parse().unwrap()),
            None => (s.parse().unwrap(), 1),
        }
    };
    let (os, ol) = range(old);
    let (ns, nl) = range(new);
    (os, ol, ns, nl)
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Best effort. I3 says zero retained temp files, and a test suite that
        // litters would be the first thing to break that claim.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
