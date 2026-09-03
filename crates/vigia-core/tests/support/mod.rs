// Each test binary compiles this module separately, so anything only one of
// them uses reads as dead code in the others.
#![allow(dead_code)]

//! A disposable git repository, built by real `git`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use vigia_core::{
    CONTEXT, Class, FileChange, Frame, FrameStats, HighlightStats, Highlighter, Samples, Worktree,
};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// An object id no repository holds, so an index entry can name a blob that is
/// not there.
pub const ABSENT_BLOB: &str = "0123456789012345678901234567890123456789";

/// The readable file every unreadable-entry fixture keeps beside the broken one,
/// so "one entry failed" and "every entry failed" are different assertions.
pub const KEPT: &str = "kept.txt";

/// The path those fixtures break.
pub const GONE: &str = "gone.txt";

/// Idle frames allowed while a fixture's writes settle.
const SETTLE_FRAMES: usize = 8;

/// How long to wait for a fixture's own writes to become provably old.
const SETTLE_WAIT: Duration = Duration::from_millis(2_500);

/// Multiplier applied to absolute wall-clock bounds, and to nothing else.
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
pub fn time(work: impl FnOnce()) -> Duration {
    timed(work).1
}

/// [`time`], for a stage that produces something the next stage needs.
pub fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = work();
    (value, start.elapsed())
}

/// [`time`], and the thread CPU time the same work spent.
pub fn time_cpu(work: impl FnOnce()) -> (Duration, Duration) {
    let (_, wall, cpu) = timed_cpu(work);
    (wall, cpu)
}

/// [`time_cpu`], for a stage that produces something the next stage needs.
pub fn timed_cpu<T>(work: impl FnOnce() -> T) -> (T, Duration, Duration) {
    let before = thread_cpu();
    let (value, wall) = timed(work);
    let cpu = match (before, thread_cpu()) {
        (Some(before), Some(after)) => after.saturating_sub(before),
        _ => wall,
    };
    (value, wall, cpu)
}

/// This thread's CPU time so far, or `None` where the platform has no clock.
#[cfg(unix)]
fn thread_cpu() -> Option<Duration> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `clock_gettime` writes a `timespec` through the pointer and reads
    // nothing else. The clock id is a constant from the same crate as the struct.
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } == 0;
    ok.then(|| Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32))
}

#[cfg(windows)]
fn thread_cpu() -> Option<Duration> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: four out-parameters, all owned here, and a pseudo-handle that needs
    // no close. The call writes only through those pointers.
    let ok = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } != 0;
    // Both halves, in units of a hundred nanoseconds. Kernel time is ours: a
    // syscall the frame makes is work the frame did.
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    ok.then(|| Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
}

#[cfg(not(any(unix, windows)))]
fn thread_cpu() -> Option<Duration> {
    None
}

/// Assert an absolute p99 budget, **re-measuring once before believing a breach**.
pub fn holds_p99(
    claim: &str,
    budget: Duration,
    first: &Samples,
    detail: impl Fn() -> String,
    mut resample: impl FnMut() -> (Duration, Duration),
) {
    let taken = first.len();
    holds_p99_rounds(claim, budget, first, detail, || {
        let mut wall = Samples::new(taken.max(1));
        let mut cpu = Samples::new(taken.max(1));
        for _ in 0..taken {
            let (one_wall, one_cpu) = resample();
            wall.push(one_wall);
            cpu.push(one_cpu);
        }
        (wall, Some(cpu))
    });
}

/// [`holds_p99`] where a round is produced whole rather than a sample at a time.
pub fn holds_p99_rounds(
    claim: &str,
    budget: Duration,
    first: &Samples,
    detail: impl Fn() -> String,
    round: impl FnOnce() -> (Samples, Option<Samples>),
) {
    let one = Shape::of(first);
    if one.p99 <= budget {
        return;
    }

    let (again, cpu) = round();
    let two = Shape::of(&again);
    if two.p99 <= budget {
        eprintln!(
            "note: {claim} breached on the first round and held on the second, \
             which is #178's stall shape: {one} then {two}, against {budget:?}. \
             {}",
            detail()
        );
        return;
    }

    // **The second round's CPU time is what decides, where the first version of
    // this guessed from the shape.** A p50 inside budget with a tail outside it is
    // *consistent with* a stall and does not establish one; thread CPU time
    // establishes it, because no amount of host contention inflates work done.
    if let Some(cpu) = cpu.as_ref() {
        let deficit = again.total().saturating_sub(cpu.total());
        // What the deficit has to explain: the round's own excess, in the same units as
        // the deficit.
        let excess = again.excess_over(budget);
        let overshoot = two.p99.saturating_sub(budget);
        // Both sides are sums over the round, and that is the whole correction, in two
        // parts. This compared `deficit`, a whole round's off-CPU time, against a
        // single frame's excess over budget.
        if deficit >= excess {
            eprintln!(
                "note: {claim} was over the {budget:?} budget on wall clock twice \
                 ({one} then {two}) and the round spent {deficit:?} **off-CPU**, \
                 which covers the {excess:?} the round spent over budget in total \
                 (p99 alone was {overshoot:?} over), so the overshoot is \
                 time this process was not running rather than work it did. Reported \
                 and not failed, and that is the whole of #178's weakening. {}",
                detail()
            );
            return;
        }
        panic!(
            "{claim} was over the {budget:?} budget twice on wall clock ({one} then \
             {two}) and the round spent only {deficit:?} off-CPU against {excess:?} \
             spent over budget across the round (p99 alone was {overshoot:?} over), \
             so the time went into **work done** and this is \
             the frame path rather than the host: contention cannot inflate a CPU \
             clock. {}",
            detail()
        );
    }

    panic!(
        "{claim} was over the {budget:?} budget twice: {one} then {two}, with no CPU \
         clock to attribute it with, so it is treated as ours. {}",
        detail()
    );
}

/// The three numbers a timed gate argues with, and how they read as a sentence.
struct Shape {
    p50: Duration,
    p99: Duration,
    max: Duration,
}

impl Shape {
    fn of(samples: &Samples) -> Self {
        Self {
            p50: samples.percentile(0.50).unwrap_or_default(),
            p99: samples.percentile(0.99).unwrap_or_default(),
            max: samples.max().unwrap_or_default(),
        }
    }
}

impl std::fmt::Display for Shape {
    /// **The spread is printed because it is the diagnosis.** A frame path on a
    /// quiet machine draws p99 within about a third of p50; a stalled sample puts
    /// the ratio into double figures, which is a fact about the host and reads
    /// straight off the line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let spread = self.p99.as_secs_f64() / self.p50.as_secs_f64().max(f64::MIN_POSITIVE);
        write!(
            f,
            "p99 {:?} (p50 {:?}, max {:?}, spread {spread:.0}x)",
            self.p99, self.p50, self.max
        )
    }
}

/// Whether the absolute wall-clock tier should assert.
pub fn absolute_gates_apply(how: &str) -> bool {
    if cfg!(debug_assertions) {
        eprintln!("note: the absolute budget gates are skipped in a debug build; run `{how}`");
        false
    } else {
        true
    }
}

/// Held for the timed region of an absolute gate, so two never overlap.
static TIMED: Mutex<()> = Mutex::new(());

pub fn exclusively_timed() -> MutexGuard<'static, ()> {
    TIMED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What git reports for one path, with binary distinguishable from unchanged.
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

/// Make a named pipe at `rela`, or report that this platform would not.
///
/// `false` is a skip and not a failure, the way [`made_link`] is: the entry
/// class exists only on unix, and not every unix filesystem will hold one.
#[cfg(unix)]
pub fn made_fifo(scratch: &Scratch, rela: &str) -> bool {
    let made = std::process::Command::new("mkfifo")
        .arg(scratch.path_of(rela))
        .status()
        .is_ok_and(|status| status.success());
    if !made {
        eprintln!(
            "note: this platform would not create the fifo {rela}, so the              blocking-read reading is unchecked here"
        );
    }
    made
}

/// The other half of non-vacuity: git has to have stored a symlink.
pub fn git_stored_a_symlink(scratch: &Scratch, link: &str) -> bool {
    let mode = scratch.index_mode(link);
    if mode == "120000" {
        return true;
    }
    // An empty mode is the caller's bug, not the platform's, so it panics rather than
    // skipping. `index_mode` shells out to `git ls-files -s`, which reports nothing at
    // all for a path that is not in the index, so asking before `commit_all` returns
    // `""` here.
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

/// Commit `link` as a mode `120000` blob and then let **git** write the
/// working-tree side.
pub fn checkout_link(scratch: &Scratch, target: &str, link: &str) -> bool {
    // Set for the same reason `Scratch::new` sets `core.autocrlf`: a developer's global
    // config decides this, and on Windows it is commonly off, which would make git
    // write a plain file and skip the one gate this fixture exists for.
    scratch.git(&["config", "core.symlinks", "true"]);
    if !committed_link(scratch, target, link) {
        return false;
    }
    // Remove the harness's link and have git write its own from the index.
    std::fs::remove_file(scratch.path_of(link)).expect("remove the harness link");
    scratch.git(&["checkout-index", "-f", "--", link]);
    if scratch
        .path_of(link)
        .symlink_metadata()
        .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return true;
    }
    eprintln!(
        "note: git checked {link} out as a regular file rather than a symlink \
         (core.symlinks off), so the separator conversion is unchecked here"
    );
    false
}

/// Link, commit, and confirm git recorded mode `120000`.
pub fn committed_link(scratch: &Scratch, target: &str, link: &str) -> bool {
    if !made_link(scratch, target, link) {
        return false;
    }
    scratch.commit_all("initial");
    git_stored_a_symlink(scratch, link)
}

/// Every change, sorted by path so assertions do not depend on walk order.
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
pub fn numbered_lines(count: usize) -> String {
    (1..=count).map(|i| format!("line {i}\n")).collect()
}

/// Where a file sits in the frame, by path rather than by position.
pub fn index_of(frame: &Frame, path: &str) -> usize {
    frame
        .files()
        .iter()
        .position(|change| change.path == path)
        .unwrap_or_else(|| {
            panic!(
                "{path} is not a changed file; the frame holds {:?}",
                frame.files().iter().map(|c| &c.path).collect::<Vec<_>>()
            )
        })
}

/// Advance one frame and fetch every diff in it.
pub fn materialise(frame: &mut Frame) {
    frame.advance().expect("advance");
    for i in 0..frame.files().len() {
        frame.diff(i).expect("diff");
    }
}

/// Wait for the frame's files to become provably unchanged, then prove they did.
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
pub fn settle_spans(frame: &mut Frame) -> u64 {
    std::thread::sleep(SETTLE_WAIT);
    let before = frame.stats().measured;
    frame.advance().expect("advance");
    frame.height(|_, _| 0).expect("height");
    frame.stats().measured - before
}

/// What one drive of the highlighter drew, in the two shapes gates ask for.
pub struct Drawn {
    /// Display rows the walk asked the highlighter for.
    pub rows: usize,
    /// The class of every span those rows produced, in draw order.
    pub classes: Vec<Class>,
}

/// Highlight `hunks` hunks of `path` starting at `first`, every line of each,
/// the way a frame standing on that part of the file does.
pub fn highlight_window(
    frame: &mut Frame,
    highlighter: &mut Highlighter,
    path: &str,
    first: usize,
    hunks: usize,
) -> Drawn {
    let index = index_of(frame, path);

    let mut pass = highlighter.pass();
    let (_, diff) = frame.diff(index).expect("diff");
    assert!(
        diff.hunks.len() >= first + hunks,
        "the fixture has {} hunks, so a window of {hunks} at {first} runs off \
         the end and the gate would measure a short window",
        diff.hunks.len()
    );

    let mut drawn = Drawn {
        rows: 0,
        classes: Vec::new(),
    };
    for (offset, hunk) in diff.hunks[first..first + hunks].iter().enumerate() {
        for line in 0..hunk.lines.len() {
            let spans = pass.spans(path, first + offset, hunk, line, None);
            drawn.classes.extend(spans.iter().map(|span| span.class));
            drawn.rows += 1;
        }
    }
    drop(pass);
    drawn
}

/// What one frame cost the highlighter, as the difference between two readings.
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

/// Wait until nothing under `root` is still moving.
pub fn settle_tree(root: &Path) {
    const STILL_FOR: Duration = Duration::from_millis(40);
    const GIVE_UP_AFTER: Duration = Duration::from_secs(10);

    let deadline = Instant::now() + GIVE_UP_AFTER;
    let mut last = tree_fingerprint(root);
    loop {
        std::thread::sleep(STILL_FOR);
        let now = tree_fingerprint(root);
        if now == last {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the fixture at {} was still changing after {GIVE_UP_AFTER:?}, so nothing \
             measured against it would be measuring the test",
            root.display()
        );
        last = now;
    }
}

/// Every entry under `root` with the two facts a write moves.
fn tree_fingerprint(root: &Path) -> Vec<(PathBuf, u64, Option<std::time::SystemTime>)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                pending.push(path.clone());
            }
            found.push((path, meta.len(), meta.modified().ok()));
        }
    }
    found.sort();
    found
}

impl Scratch {
    /// Create an initialised repository with deterministic config.
    pub fn new(name: &str) -> Self {
        Self::in_dir(&std::env::temp_dir(), name)
    }

    /// The same thing, somewhere other than the temp directory.
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
        // **These two are here for `crates/vigia/tests/writes.rs`, which asserts
        // that nothing under the worktree moves while `vigia` runs.** With
        // `core.fsmonitor` on, a `git add` spawns a `git-fsmonitor--daemon` that
        // outlives the command and keeps writing inside `.git`, including a
        // `cookies/` directory it creates and deletes to synchronise with clients.
        scratch.git(&["config", "core.fsmonitor", "false"]);
        scratch.git(&["config", "core.untrackedCache", "false"]);
        scratch
    }

    /// A repository configured the way a Windows checkout is, so that git's
    /// clean filter has something to do.
    pub fn crlf_worktree(name: &str, attributes: Option<&str>) -> Self {
        let scratch = Self::new(name);
        scratch.git(&["config", "core.autocrlf", "true"]);
        if let Some(attributes) = attributes {
            scratch.write(".gitattributes", attributes);
        }
        scratch
    }

    /// Write a file with CRLF terminators, the way an editor on Windows does.
    pub fn write_crlf(&self, rela: &str, contents: &str) {
        assert!(
            !contents.contains('\r'),
            "{rela} already carries a carriage return, so converting would double it"
        );
        self.write(rela, contents.replace('\n', "\r\n"));
    }

    /// A repository whose working tree differs from its index by every line of
    /// every file: `2 * files * lines` changed lines in total.
    pub fn large_diff(name: &str, files: usize, lines: usize) -> Self {
        let scratch = Self::new(name);
        scratch.fill_large_diff(files, lines);
        scratch
    }

    /// The body of [`Scratch::large_diff`], for a repository that already
    /// exists because it had to be created somewhere specific.
    pub fn fill_large_diff(&self, files: usize, lines: usize) {
        self.fill_pairs(
            files,
            |f| format!("src/mod_{f}.rs"),
            |tag| generated(lines, tag),
        );
    }

    /// Write `files` files, commit them, then rewrite every one line for line.
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

    /// A repository holding a nested checkout, plus one ordinary edit beside it.
    ///
    /// `git status` reports the nested repository as a single untracked entry
    /// naming the directory itself, with nothing to distinguish it from a file.
    pub fn with_nested_repository(name: &str) -> Self {
        let scratch = Self::new(name);
        scratch.write(
            KEPT, "one
",
        );
        scratch.commit_all("baseline");
        scratch.write(
            KEPT, "one
two
",
        );
        scratch.git(&["init", "-q", "nested"]);
        scratch.write(
            "nested/inner.txt",
            "inner
",
        );
        scratch
    }

    /// A repository where one path's left-hand side is a blob the object
    /// database does not hold, plus one ordinary edit beside it.
    ///
    /// This is a per-file failure that is **not** a directory, which is what
    /// separates the two halves of the containment. Built from git rather than
    /// from permissions, because a mode that denies a read is not portable and
    /// this is.
    pub fn with_a_missing_blob(name: &str) -> Self {
        let scratch = Self::new(name);
        scratch.write(
            GONE, "one
",
        );
        scratch.write(
            KEPT, "one
",
        );
        scratch.commit_all("baseline");
        scratch.write(
            GONE, "one
two
",
        );
        scratch.write(
            KEPT, "one
two
",
        );
        scratch.point_at_a_missing_blob(GONE);
        scratch
    }

    /// Point one index entry at [`ABSENT_BLOB`], leaving the worktree alone.
    pub fn point_at_a_missing_blob(&self, rela: &str) {
        self.git(&[
            "update-index",
            "--cacheinfo",
            &format!("100644,{ABSENT_BLOB},{rela}"),
        ]);
    }

    /// A repository of long lines mixing Japanese, emoji and Latin.
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
    pub fn index_mode(&self, rela: &str) -> String {
        let out = self.git(&["ls-files", "-s", "--", rela]);
        out.split_whitespace().next().unwrap_or_default().to_owned()
    }

    /// Replace one line of a file, leaving every other byte alone.
    pub fn edit_line(&self, rela: &str, line: usize, text: &str) {
        self.rewrite(rela, |lines| {
            lines[line] = text.to_owned();
        });
    }

    /// Change one character of a line, keeping the file's length identical.
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

    /// Wait until nothing under the fixture is still moving.
    #[must_use]
    pub fn settled(self) -> Self {
        settle_tree(self.root());
        self
    }

    /// Open the working tree through the crate under test.
    pub fn worktree(&self) -> Worktree {
        Worktree::discover(&self.path).expect("discover scratch repository")
    }

    /// Put a file on disk the way `git checkout` would, rather than the way the
    /// test wrote it.
    pub fn checkout(&self, rela: &str) {
        std::fs::remove_file(self.path_of(rela)).expect("remove before checkout");
        self.git(&["checkout", "--", rela]);
    }

    /// What `git diff --numstat` says about one path.
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
    pub fn git_hunk_headers(&self, rela: &str) -> Vec<(u32, u32, u32, u32)> {
        let out = self.git(&["diff", "-U3", "--", rela]);
        out.lines()
            .filter(|line| line.starts_with("@@"))
            .map(parse_hunk_header)
            .collect()
    }
}

/// Plausible source lines, distinct on both sides so every line differs.
pub fn generated(lines: usize, tag: &str) -> String {
    joined(lines, tag, generated_line)
}

/// `lines` lines from `line`, each newline terminated.
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
fn generated_line(at: usize, tag: &str) -> String {
    let n = at + 1;
    format!("fn {tag}_{n}() {{ let value = {}; }}", n * 7)
}

/// Extension the wide fixture uses by default.
pub const WIDE_EXT: &str = "md";

/// An extension `syntect` has no grammar for.
pub const WIDE_UNPARSED_EXT: &str = "vigia";

/// The repeating unit of a [`wide_line`], in source characters.
pub const WIDE_UNIT_CHARS: usize = 50;

/// The same unit in terminal columns: 28 + 2 + 35.
pub const WIDE_UNIT_COLUMNS: usize = 65;

/// Units per line.
pub const WIDE_UNITS: usize = 8;

/// Long lines of mixed Japanese, emoji and Latin, one per line, newline
/// terminated.
pub fn wide_generated(lines: usize, tag: &str) -> String {
    joined(lines, tag, wide_line)
}

/// The one line [`wide_generated`] writes at `at`, with no line ending.
///
/// **The shape, stated rather than left to be counted**, because
/// The first acceptance
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

/// The extension the prose fixture is written with.
pub const PROSE_EXT: &str = "md";

/// Inline code spans per line of a [`prose_generated`] line.
///
/// **The load-bearing parameter of the fixture, and the reason it is a named
/// constant rather than a literal in a loop.** The cost this fixture exists to
/// measure is exponential in this number, not linear. Measured against the
/// unguarded grammar, one line of exactly this shape parsed on its own:
///
/// | spans | 4 | 5 | **6** | 7 | 8 |
/// |---|---|---|---|---|---|
/// | ms | 0.803 | 3.040 | **12.006** | 16.884 | 16.686 |
///
/// So an edit that drifts this from 6 to 4 does not weaken the gate by a third,
/// it weakens it by **fifteen times**, and every assertion downstream still
/// passes.
///
/// Six rather than seven or eight because seven is already the plateau, where
/// `fancy-regex`'s 1,000,000 backtrack limit rather than the pattern is what
/// bounds the number: 7 and 8 differ by less than 1.2%, so a change that made
/// the pattern twice as expensive would not move them. Six is the largest value
/// still on the steep part of the curve. It is not chosen for strength, which
/// both would have: in the frame, six spans breach at **102.39ms p99 against
/// the 16ms budget** and seven at 103.36ms, and the guard takes six to 1.11ms.
pub const PROSE_SPANS: usize = 6;

/// **The floor, checked at compile time rather than by a test.**
const _: () = assert!(
    PROSE_SPANS >= 6,
    "PROSE_SPANS is below the 6 the fixture was calibrated at. The curve, \
     measured against the unguarded grammar on one line of this exact shape: \
     4 spans 0.803ms, 5 spans 3.040ms, 6 spans 12.006ms. Below 6 the unguarded \
     frame stops breaching the 16ms budget, and the gate that depends on it \
     passes whether or not the guard is present."
);

/// Markdown prose carrying [`PROSE_SPANS`] inline code spans per line and **no
/// pipe character**, newline terminated.
pub fn prose_generated(lines: usize, tag: &str) -> String {
    // One line per paragraph, and the blank line between them is the fixture. Markdown
    // runs its block-start lookahead, which is where the table-row test lives, only on
    // the *first* line of a block: continuation lines of a paragraph take a much
    // cheaper inline path.
    (0..lines)
        .map(|at| format!("{}\n\n", prose_line(at, tag)))
        .collect()
}

/// The one line [`prose_generated`] writes at `at`, with no line ending.
fn prose_line(at: usize, tag: &str) -> String {
    // It may not begin `N. `, and that is the whole reason this is spelled out rather
    // than reusing [`generated_line`]'s prefix. A leading ordinal and a period is an
    // *ordered list marker* in Markdown, so a fixture written that way is a list and
    // takes the block-start path a list takes, not the one ordinary prose takes.
    let mut line = format!("Line {at} of the {tag} frame path calls ");
    for span in 0..PROSE_SPANS {
        line.push_str(&format!("`sym_{at}_{span}` and "));
    }
    line.push_str("then reports what it drew to the pane.");
    line
}

impl Scratch {
    /// A repository of Markdown prose files, every line carrying
    /// [`PROSE_SPANS`] code spans.
    pub fn prose_lines_as(name: &str, files: usize, lines: usize, ext: &str) -> Self {
        let scratch = Self::new(name);
        scratch.fill_pairs(
            files,
            |f| format!("docs/prose_{f}.{ext}"),
            |tag| prose_generated(lines, tag),
        );
        scratch
    }
}
