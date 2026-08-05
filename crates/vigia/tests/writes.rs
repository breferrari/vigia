//! `SPEC.md` §11.1: the monitor writes nothing.
//!
//! The rule is older than the line that states it. §6 rejects an active probe of
//! the filesystem's timestamp granularity because *"a read-only monitor will not
//! do"* that, and §10's settle-margin bullet leans on *"a monitor never writes"*
//! to explain why the margin cannot be narrowed from a sample. Both are load
//! bearing and neither had a gate, so the property they rest on was true by
//! nobody's decision: `vigia` happens to open no file for writing, and a cache, a
//! lock file or a crash dump added in good faith would have made two recorded
//! arguments quietly wrong.
//!
//! B7 is what made that unaffordable. The ruling against an observation log is
//! *"the monitor does not write"*, so a ruling with no gate under it would be the
//! same shape §7 keeps finding: settled-looking and proving nothing.
//!
//! **What this holds and what it cannot.** The subject is every entry under the
//! worktree root, `.git` included, across a first paint, six reader actions, a
//! second tick and the grammar warmer. `Session::enter` is out, which is the
//! carve-out `budgets.rs`, `soak.rs` and `first_paint.rs` already name: it needs a
//! tty. So a write from inside the terminal takeover is outside this gate, and I8
//! is what covers that path.
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
use vigia_core::{Frame, Highlighter, History, WARM_FILES, Worktree};

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
    /// removing `src/MUTATION-transient` inside the driven window turns the gate
    /// red on `src` alone, at 4096 bytes both sides with the modification time
    /// 3.8ms apart. NTFS, and the limit is the one §6 already documents: a create
    /// and delete inside a single timestamp granule would leave both maps
    /// agreeing, so this is a strong catch rather than a proof.
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
fn stamps(root: &Path) -> BTreeMap<PathBuf, Stamp> {
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
            if stamp.dir {
                pending.push(path.clone());
            }
            let rela = path
                .strip_prefix(root)
                .expect("an entry below the root it was walked from")
                .to_path_buf();
            found.insert(rela, stamp);
        }
    }
    found
}

/// One entry's stamp, read the way [`Stamp`] documents.
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
fn drive(root: &Path) {
    let worktree = Worktree::discover(root).expect("discover");
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let mut highlighter = Highlighter::new();
    let mut app = App::new();
    let history = History::new();
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());

    let mut paint = |app: &mut App, frame: &mut Frame, highlighter: &mut Highlighter| {
        let chrome = app.chrome("fixture", None);
        let body = body_layout(area(), &chrome, frame.files().len());
        let view = app
            .view(frame, highlighter, &history, body)
            .expect("collect a view");
        render(&mut buf, area(), &view, &theme, &chrome);
        body.diff
    };

    let height = paint(&mut app, &mut frame, &mut highlighter);
    for action in [
        Action::Scroll(12),
        Action::Page(1),
        Action::Bottom,
        Action::Top,
        Action::ToggleFollow,
        Action::ScrollList(3),
    ] {
        app.apply(action, &mut frame, height)
            .expect("apply a reader action");
        paint(&mut app, &mut frame, &mut highlighter);
    }

    frame.advance().expect("advance a second time");
    paint(&mut app, &mut frame, &mut highlighter);

    // **Joined rather than detached, which is the opposite of what `run` does and
    // is right here.** `run` drops the handle because nothing needs the result;
    // this gate needs the thread to have *finished* before the second stamp, or a
    // write it made would land after the comparison and the gate would pass by
    // racing. It is the one stage that both spawns a thread and touches the
    // filesystem, so it is the last place a write could hide.
    let warmer = highlighter.warm_ahead(
        worktree.workdir().to_path_buf(),
        frame
            .files()
            .iter()
            .take(WARM_FILES)
            .map(|change| change.path.clone())
            .collect(),
    );
    let warmed = warmer.join().expect("the warmer finished");
    assert!(
        warmed > 0,
        "the warmer compiled nothing, so the one stage that spawns a thread was \
         not exercised and this gate is a paint test wearing its name"
    );
}

#[test]
fn the_monitor_writes_nothing_while_it_runs() {
    let scratch = Scratch::large_diff("writes-nothing", FILES, LINES);
    let root = scratch.root().to_path_buf();

    let before = stamps(&root);
    assert!(
        before.len() > FILES,
        "the fixture stamped {} entries, which is fewer than its own files, so \
         the walk is not seeing the tree",
        before.len()
    );

    drive(&root);

    let moved = difference(&before, &stamps(&root));
    assert!(
        moved.is_empty(),
        "SPEC.md §11.1: the monitor writes nothing, and this run moved {} \
         entries:\n{}",
        moved.len(),
        moved.join("\n")
    );
}

#[test]
fn the_stamp_sees_a_write_that_does_happen() {
    let scratch = Scratch::large_diff("writes-detected", FILES, LINES);
    let root = scratch.root().to_path_buf();

    let before = stamps(&root);

    // **A different length, deliberately.** Two writes of the same length inside
    // one modification-time granule are indistinguishable by `stat`, which is
    // §6's racily-clean case and is the flake a stamp resting on the clock alone
    // would be. Growing the file takes the comparison off that axis entirely.
    scratch.write("src/mod_0.rs", "one line\n".repeat(LINES * 2));
    scratch.write("untracked.txt", "and a file that was not there before\n");

    let moved = difference(&before, &stamps(&root));
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
