// Each test binary compiles this module separately, so anything only one of
// them uses reads as dead code in the others.
#![allow(dead_code)]

//! A disposable git repository, built by real `git`.
//!
//! Fixtures are created by shelling out rather than by `gix` on purpose. The
//! open question Phase 1 answers is whether `gix` reads a working tree the way
//! git wrote it, and a fixture written by the library under test cannot answer
//! that.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use vigia_core::{CONTEXT, Frame, FrameStats, HighlightStats, Worktree};

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
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
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

    /// A repository whose working tree differs from its index by every line of
    /// every file: `2 * files * lines` changed lines in total.
    ///
    /// Used to size fixtures against the budgets, which are written in lines of
    /// diff rather than in files.
    pub fn large_diff(name: &str, files: usize, lines: usize) -> Self {
        let scratch = Self::new(name);
        for f in 0..files {
            scratch.write(&format!("src/mod_{f}.rs"), generated(lines, "before"));
        }
        scratch.commit_all("baseline");
        for f in 0..files {
            scratch.write(&format!("src/mod_{f}.rs"), generated(lines, "after"));
        }
        scratch
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
fn generated(lines: usize, tag: &str) -> String {
    (0..lines)
        .map(|at| {
            let mut line = generated_line(at, tag);
            line.push('\n');
            line
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
