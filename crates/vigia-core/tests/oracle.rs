//! `vigia-core` against git, over generated sequences of worktree operations.
//!
//! Each case builds a repository, applies a few operations, and compares the
//! changed set, the rename pairs and every text hunk with git's own answer.
//! `PROPTEST_CASES` raises the case count; `VIGIA_ORACLE_STRICT=1` makes the
//! hunk-header comparison fatal.

mod support;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngAlgorithm, RngSeed};
use support::Scratch;
use vigia_core::{ChangeKind, ChangeOptions, FileChange, LineKind, Origin, Worktree};

/// The paths a case can touch: ASCII, two directories, none equal to another
/// but for case.
const NAMES: [&str; 8] = [
    "d0/a.txt", "d0/b.txt", "d0/c.rs", "d0/d.md", "d1/e.txt", "d1/f.bin", "d1/g.txt", "d1/h.txt",
];

/// The attribute rules a case can write, the last removing the file.
const RULES: [&str; 4] = [
    "*.bin binary\n",
    "*.md -diff\n",
    "*.txt text eol=crlf\n",
    "",
];

const WORDS: [&str; 12] = [
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet",
    "kilo", "lima",
];

/// A case slower than this fails, naming its operations.
const CASE_CEILING: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
enum Op {
    Write {
        file: usize,
        seed: u8,
        lines: u8,
    },
    Edit {
        file: usize,
        line: u8,
        seed: u8,
    },
    Insert {
        file: usize,
        line: u8,
        seed: u8,
    },
    DeleteLine {
        file: usize,
        line: u8,
    },
    Remove {
        file: usize,
    },
    Rename {
        from: usize,
        to: usize,
    },
    /// A rename that keeps `keep` of every 12 lines, so pairs land on both
    /// sides of git's similarity threshold.
    Move {
        from: usize,
        to: usize,
        keep: u8,
        seed: u8,
    },
    Stage {
        file: usize,
    },
    StageAll,
    Binary {
        file: usize,
        seed: u8,
    },
    /// A binary rename that keeps `keep` of every 12 blocks of 256 bytes.
    MoveBinary {
        from: usize,
        to: usize,
        keep: u8,
        seed: u8,
    },
    Attributes {
        rule: usize,
    },
    Lfs {
        file: usize,
        seed: u8,
    },
}

fn op() -> impl Strategy<Value = Op> {
    let file = 0..NAMES.len();
    prop_oneof![
        3 => (file.clone(), any::<u8>(), 1..40u8).prop_map(|(file, seed, lines)| Op::Write { file, seed, lines }),
        4 => (file.clone(), any::<u8>(), any::<u8>()).prop_map(|(file, line, seed)| Op::Edit { file, line, seed }),
        2 => (file.clone(), any::<u8>(), any::<u8>()).prop_map(|(file, line, seed)| Op::Insert { file, line, seed }),
        2 => (file.clone(), any::<u8>()).prop_map(|(file, line)| Op::DeleteLine { file, line }),
        1 => file.clone().prop_map(|file| Op::Remove { file }),
        2 => (file.clone(), file.clone()).prop_map(|(from, to)| Op::Rename { from, to }),
        2 => (file.clone(), file.clone(), 0..13u8, any::<u8>()).prop_map(|(from, to, keep, seed)| Op::Move { from, to, keep, seed }),
        2 => file.clone().prop_map(|file| Op::Stage { file }),
        1 => Just(Op::StageAll),
        1 => (file.clone(), any::<u8>()).prop_map(|(file, seed)| Op::Binary { file, seed }),
        2 => (file.clone(), file.clone(), 0..13u8, any::<u8>())
            .prop_map(|(from, to, keep, seed)| Op::MoveBinary { from, to, keep, seed }),
        1 => (0..RULES.len()).prop_map(|rule| Op::Attributes { rule }),
        1 => (file, any::<u8>()).prop_map(|(file, seed)| Op::Lfs { file, seed }),
    ]
}

/// 3 KiB with no line break and a NUL, so both sides call it binary.
fn binary(seed: u8) -> Vec<u8> {
    (0..3072u32)
        .map(|at| (at as u8).wrapping_mul(seed | 1).wrapping_add(seed) | 1)
        .chain([0])
        .collect()
}

fn text_line(seed: u8, at: usize) -> String {
    let pick = |n: usize| WORDS[(usize::from(seed) * 7 + at * 5 + n) % WORDS.len()];
    format!("{} {} {}", pick(0), pick(3), at % 9)
}

fn text(seed: u8, lines: u8) -> String {
    (0..usize::from(lines))
        .map(|at| text_line(seed, at) + "\n")
        .collect()
}

fn config(cases: u32, seed: u64) -> ProptestConfig {
    ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(cases),
        rng_seed: RngSeed::Fixed(seed),
        rng_algorithm: RngAlgorithm::ChaCha,
        max_shrink_time: 30_000,
        failure_persistence: std::env::var_os("PROPTEST_DISABLE_FAILURE_PERSISTENCE")
            .is_none()
            .then(|| {
                Box::new(FileFailurePersistence::Direct(
                    "proptest-regressions/oracle.txt",
                )) as Box<dyn proptest::test_runner::FailurePersistence>
            }),
        ..ProptestConfig::default()
    }
}

/// git with the diff options the oracle pins, in `dir`, with `index` as its
/// index when given.
fn git(dir: &Path, index: Option<&Path>, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "diff.algorithm=myers",
            "-c",
            "diff.indentHeuristic=true",
            "-c",
            "diff.renames=true",
            "-c",
            "status.renames=true",
            "-c",
            "core.quotePath=false",
        ])
        .args(args)
        .current_dir(dir);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let out = command.output().expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn apply(scratch: &Scratch, op: &Op) {
    let read = |file: usize| std::fs::read_to_string(scratch.path_of(NAMES[file])).ok();
    let lines_of = |file: usize| -> Option<Vec<String>> {
        read(file).map(|text| text.lines().map(str::to_owned).collect())
    };
    let write_lines = |file: usize, lines: &[String]| {
        let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
        scratch.write(NAMES[file], body);
    };
    match *op {
        Op::Write { file, seed, lines } => scratch.write(NAMES[file], text(seed, lines)),
        Op::Edit { file, line, seed } => {
            if let Some(mut lines) = lines_of(file).filter(|lines| !lines.is_empty()) {
                let at = usize::from(line) % lines.len();
                lines[at] = text_line(seed, at + 100);
                write_lines(file, &lines);
            }
        }
        Op::Insert { file, line, seed } => {
            if let Some(mut lines) = lines_of(file) {
                let at = usize::from(line) % (lines.len() + 1);
                lines.insert(at, text_line(seed, at + 200));
                write_lines(file, &lines);
            }
        }
        Op::DeleteLine { file, line } => {
            if let Some(mut lines) = lines_of(file).filter(|lines| !lines.is_empty()) {
                lines.remove(usize::from(line) % lines.len());
                write_lines(file, &lines);
            }
        }
        Op::Remove { file } => {
            let _ = std::fs::remove_file(scratch.path_of(NAMES[file]));
        }
        Op::Rename { from, to } => {
            if from != to && scratch.path_of(NAMES[from]).exists() {
                std::fs::rename(scratch.path_of(NAMES[from]), scratch.path_of(NAMES[to]))
                    .expect("rename");
            }
        }
        // A path gone from the disk and the index stages nothing, and git
        // says so by failing.
        Op::Move {
            from,
            to,
            keep,
            seed,
        } => {
            if from != to
                && let Some(lines) = lines_of(from)
            {
                let moved: Vec<String> = lines
                    .iter()
                    .enumerate()
                    .map(|(at, line)| {
                        if at % 12 < usize::from(keep) {
                            line.clone()
                        } else {
                            text_line(seed, at + 300)
                        }
                    })
                    .collect();
                let _ = std::fs::remove_file(scratch.path_of(NAMES[from]));
                write_lines(to, &moved);
            }
        }
        Op::Stage { file } => {
            scratch.git_may_fail(&["add", "-A", "--", NAMES[file]]);
        }
        Op::StageAll => {
            git(&scratch.path_of(""), None, &["add", "-A"]);
        }
        Op::Binary { file, seed } => scratch.write(NAMES[file], binary(seed)),
        Op::MoveBinary {
            from,
            to,
            keep,
            seed,
        } => {
            if from != to
                && let Ok(bytes) = std::fs::read(scratch.path_of(NAMES[from]))
            {
                let fresh = binary(seed);
                let moved: Vec<u8> = bytes
                    .iter()
                    .enumerate()
                    .map(|(at, byte)| {
                        if (at / 256) % 12 < usize::from(keep) {
                            *byte
                        } else {
                            fresh[at % fresh.len()]
                        }
                    })
                    .collect();
                let _ = std::fs::remove_file(scratch.path_of(NAMES[from]));
                scratch.write(NAMES[to], moved);
            }
        }
        Op::Attributes { rule } => {
            if RULES[rule].is_empty() {
                let _ = std::fs::remove_file(scratch.path_of(".gitattributes"));
            } else {
                scratch.write(".gitattributes", RULES[rule]);
            }
        }
        Op::Lfs { file, seed } => {
            let oid: String = (0..32)
                .map(|i| format!("{:02x}", seed.wrapping_add(i)))
                .collect();
            scratch.write(
                NAMES[file],
                format!(
                    "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
                    100 + u32::from(seed)
                ),
            );
        }
    }
}

/// One change as both sides report it: its letter, path, and source.
type Entry = (char, String, Option<String>);

fn ours(change: &FileChange) -> Entry {
    let (letter, from) = match &change.kind {
        ChangeKind::Added => ('A', None),
        ChangeKind::Modified => ('M', None),
        ChangeKind::Removed => ('D', None),
        ChangeKind::Renamed { from } => ('R', Some(from.clone())),
        ChangeKind::Copied { from } => ('C', Some(from.clone())),
        ChangeKind::TypeChange => ('T', None),
        other => panic!(
            "{} is {other:?}, which no generated operation makes",
            change.path
        ),
    };
    (letter, change.path.clone(), from)
}

/// `--name-status -z` output, as entries.
fn theirs(raw: &[u8]) -> BTreeSet<Entry> {
    let text = String::from_utf8_lossy(raw);
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    let mut entries = BTreeSet::new();
    while let Some(status) = fields.next() {
        let letter = status.chars().next().expect("a status letter");
        if matches!(letter, 'R' | 'C') {
            let from = fields.next().expect("a source").to_owned();
            let to = fields.next().expect("a destination").to_owned();
            entries.insert((letter, to, Some(from)));
        } else {
            entries.insert((letter, fields.next().expect("a path").to_owned(), None));
        }
    }
    entries
}

/// Paths `git status --porcelain=v2 -z` reports changed, staged and unstaged,
/// with a rename counted as both of its paths.
fn status_paths(raw: &[u8]) -> (BTreeSet<String>, BTreeSet<String>) {
    let text = String::from_utf8_lossy(raw);
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    let (mut staged, mut unstaged) = (BTreeSet::new(), BTreeSet::new());
    while let Some(entry) = fields.next() {
        let parts: Vec<&str> = entry.split(' ').collect();
        match parts[0] {
            "?" => {
                unstaged.insert(entry[2..].to_owned());
            }
            "1" | "2" => {
                let xy: Vec<char> = parts[1].chars().collect();
                let path = parts[if parts[0] == "1" { 8 } else { 9 }..].join(" ");
                let from = (parts[0] == "2").then(|| fields.next().expect("a source").to_owned());
                if xy[0] != '.' {
                    staged.insert(path.clone());
                    staged.extend(from.clone());
                }
                if xy[1] != '.' {
                    unstaged.insert(path);
                    unstaged.extend(from);
                }
            }
            other => panic!("status reported {other:?}, which no generated operation makes"),
        }
    }
    (staged, unstaged)
}

fn paths(entries: &BTreeSet<Entry>) -> BTreeSet<String> {
    entries
        .iter()
        .flat_map(|(_, path, from)| std::iter::once(path.clone()).chain(from.clone()))
        .collect()
}

/// `--numstat -z` output by destination path: added and removed, or `None`
/// for a binary file.
fn numstat(raw: &[u8]) -> std::collections::BTreeMap<String, Option<(i64, i64)>> {
    let text = String::from_utf8_lossy(raw);
    let mut fields = text.split('\0');
    let mut found = std::collections::BTreeMap::new();
    while let Some(head) = fields.next() {
        if head.is_empty() {
            continue;
        }
        let mut parts = head.splitn(3, '\t');
        let (added, removed, path) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        let path = if path.is_empty() {
            let _from = fields.next();
            fields.next().expect("a destination").to_owned()
        } else {
            path.to_owned()
        };
        let counts = added.parse().ok().zip(removed.parse().ok());
        found.insert(path, counts);
    }
    found
}

fn lines_of(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The unstaged comparison as git sees it: everything staged into a scratch
/// index, against the tree of the real one.
struct Unstaged {
    index: std::path::PathBuf,
    tree: String,
}

fn check(scratch: &Scratch, ops: &[Op]) {
    let root = scratch.path_of("");
    let dir = root.as_path();
    let tree = String::from_utf8(git(dir, None, &["write-tree"]))
        .expect("a tree id")
        .trim()
        .to_owned();
    let index = scratch.path_of(".git/oracle-index");
    std::fs::copy(scratch.path_of(".git/index"), &index).expect("copy the index");
    git(dir, Some(&index), &["add", "-A"]);
    let unstaged = Unstaged { index, tree };

    let (status_staged, status_unstaged) = status_paths(&git(
        dir,
        None,
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
    ));
    let worktree = Worktree::discover(dir).expect("a worktree");

    for origin in [Origin::Unstaged, Origin::Staged] {
        let changes: Vec<FileChange> = worktree
            .changes_of(origin, ChangeOptions::default())
            .expect("a status walk")
            .collect::<Result<_, _>>()
            .expect("every change");
        let (index, base): (Option<&Path>, &str) = match origin {
            Origin::Unstaged => (Some(&unstaged.index), &unstaged.tree),
            Origin::Staged => (None, "HEAD"),
        };
        let theirs = theirs(&git(
            dir,
            index,
            &["diff-index", "--cached", "-M", "--name-status", "-z", base],
        ));
        let ours: BTreeSet<Entry> = changes.iter().map(ours).collect();
        assert_eq!(
            ours, theirs,
            "{origin:?} changes differ from git's after {ops:?}"
        );
        let status = match origin {
            Origin::Unstaged => &status_unstaged,
            Origin::Staged => &status_staged,
        };
        assert_eq!(
            &paths(&ours),
            status,
            "{origin:?} paths differ from git status after {ops:?}"
        );

        let counts = numstat(&git(
            dir,
            index,
            &["diff-index", "--cached", "-M", "--numstat", "-z", base],
        ));
        let minimal = numstat(&git(
            dir,
            index,
            &[
                "diff-index",
                "--cached",
                "-M",
                "--minimal",
                "--numstat",
                "-z",
                base,
            ],
        ));
        let patch = String::from_utf8_lossy(&git(
            dir,
            index,
            &["diff-index", "--cached", "-M", "-p", "-U3", base],
        ))
        .into_owned();
        for change in &changes {
            hunks(
                scratch, &worktree, change, origin, index, base, &counts, &minimal, &patch, ops,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn hunks(
    scratch: &Scratch,
    worktree: &Worktree,
    change: &FileChange,
    origin: Origin,
    index: Option<&Path>,
    base: &str,
    counts: &std::collections::BTreeMap<String, Option<(i64, i64)>>,
    minimal: &std::collections::BTreeMap<String, Option<(i64, i64)>>,
    patch: &str,
    ops: &[Op],
) {
    let path = &change.path;
    let diff = worktree.diff(change).expect("a diff");
    let theirs = counts
        .get(path)
        .unwrap_or_else(|| panic!("git has no numstat for {path}"));
    assert_eq!(
        diff.binary,
        theirs.is_none(),
        "{path} binary differs from git after {ops:?}"
    );
    let Some((added, removed)) = *theirs else {
        return;
    };
    let root = scratch.path_of("");
    let dir = root.as_path();
    let from = change.kind.source().unwrap_or(path);
    let old = match change.kind {
        ChangeKind::Added => Vec::new(),
        _ => lines_of(&git(
            dir,
            None,
            &[
                "show",
                &format!(
                    "{}:{from}",
                    if origin == Origin::Staged { "HEAD" } else { "" }
                ),
            ],
        )),
    };
    let new = match change.kind {
        ChangeKind::Removed => Vec::new(),
        _ => lines_of(&git(dir, index, &["show", &format!(":{path}")])),
    };

    // The hunks rebuild the new side from the old, and every line they keep
    // or remove is the old side's.
    let mut rebuilt = Vec::new();
    let mut at = 0usize;
    for hunk in &diff.hunks {
        let start = (hunk.old_start as usize).saturating_sub(1).max(at);
        rebuilt.extend_from_slice(&old[at..start.min(old.len())]);
        at = start;
        for line in &hunk.lines {
            match line.kind {
                LineKind::Added => rebuilt.push(line.text.clone()),
                LineKind::Context | LineKind::Removed => {
                    assert_eq!(
                        old.get(at),
                        Some(&line.text),
                        "{path}: a hunk line is not the old side's line {} after {ops:?}",
                        at + 1
                    );
                    if line.kind == LineKind::Context {
                        rebuilt.push(line.text.clone());
                    }
                    at += 1;
                }
            }
        }
    }
    rebuilt.extend_from_slice(&old[at.min(old.len())..]);
    assert_eq!(
        rebuilt, new,
        "{path}: the hunks do not rebuild the new side after {ops:?}"
    );

    let ours_added = i64::from(diff.added);
    let ours_removed = i64::from(diff.removed);
    assert_eq!(
        ours_added - ours_removed,
        added - removed,
        "{path}: net lines differ from git after {ops:?}"
    );
    if let Some(Some((least, _))) = minimal.get(path) {
        assert!(
            ours_added >= *least,
            "{path}: fewer lines added than git's minimal diff after {ops:?}"
        );
    }
    let _ = base;

    // Hunk headers against git's, reported unless strict.
    // Git writes a count of one as nothing, and a file's section opens on its
    // own `diff --git a/<from> b/<path>` line.
    let range = |start: u32, count: u32| {
        if count == 1 {
            format!("{start}")
        } else {
            format!("{start},{count}")
        }
    };
    let headers: Vec<String> = diff
        .hunks
        .iter()
        .map(|hunk| {
            format!(
                "@@ -{} +{} @@",
                range(hunk.old_start, hunk.old_lines),
                range(hunk.new_start, hunk.new_lines)
            )
        })
        .collect();
    let opening = format!("a/{from} b/{path}\n");
    let section = patch
        .split("diff --git ")
        .find(|section| section.starts_with(&opening))
        .unwrap_or("");
    let git_headers: Vec<String> = section
        .lines()
        .filter(|line| line.starts_with("@@ "))
        .map(|line| line.split(" @@").next().unwrap_or(line).to_owned() + " @@")
        .collect();
    if headers != git_headers {
        let message =
            format!("{path}: hunk headers {headers:?} where git has {git_headers:?} after {ops:?}");
        assert!(
            std::env::var_os("VIGIA_ORACLE_STRICT").is_none(),
            "{message}"
        );
        eprintln!("oracle: {message}");
    }
}

fn run(ops: &[Op], crlf: bool) {
    let began = Instant::now();
    let scratch = Scratch::new("oracle");
    if crlf {
        scratch.git(&["config", "core.autocrlf", "true"]);
    } else {
        scratch.git(&["config", "core.eol", "lf"]);
    }
    for (at, name) in NAMES.iter().enumerate() {
        scratch.write(name, text(at as u8, 12));
    }
    scratch.commit_all("base");
    for op in ops {
        apply(&scratch, op);
    }
    check(&scratch, ops);
    assert!(
        began.elapsed() < CASE_CEILING,
        "a case took {:?} after {ops:?}",
        began.elapsed()
    );
}

proptest! {
    #![proptest_config(config(64, 0x0076_6967_6961))]
    #[test]
    fn oracle(ops in proptest::collection::vec(op(), 1..12)) {
        run(&ops, false);
    }
}

proptest! {
    #![proptest_config(config(16, 0x6372_6c66))]
    #[test]
    fn oracle_crlf(ops in proptest::collection::vec(op(), 1..12)) {
        run(&ops, true);
    }
}
