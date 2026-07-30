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

use vigia_core::{Frame, FrameStats, Worktree};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Idle frames allowed while a fixture's writes settle.
///
/// Generous because a spare frame costs milliseconds, and because the assertion
/// [`settle`] ends with is what gives the number teeth.
const SETTLE_FRAMES: usize = 8;

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

/// Drive the frame until it stops re-reading, then prove that it stopped.
///
/// A file written microseconds ago cannot yet be *proved* unchanged: its
/// modification time is not strictly older than the read that fingerprinted it,
/// which is the racily-clean rule doing its job. Fixtures are written
/// immediately before a test runs, so the first frames legitimately re-read
/// them. I2a is a claim about the frame after an edit has landed, not the frame
/// racing the write, so measurement starts here.
///
/// This cannot wait out a broken cache. A frame path that never reuses anything
/// never settles, and the panic below is what it gets.
pub fn settle(frame: &mut Frame) {
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
    (1..=lines)
        .map(|n| format!("fn {tag}_{n}() {{ let value = {}; }}\n", n * 7))
        .collect()
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
