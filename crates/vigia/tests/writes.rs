//! `SPEC.md` §11.1: the monitor writes nothing.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{Action, App, Glyphs, PaintStats, Pointing, Theme, body_layout, render};
use vigia_core::{Frame, Highlighter, History, WARM_FILES, WatchOptions, Worktree};

use support::{Scratch, made_link, settle_tree};

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
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    /// Directories are stamped too, which is what catches a file created **and
    /// deleted** inside one window: adding or removing a child moves the parent's
    /// own modification time, and the file itself is in neither map.
    dir: bool,
    len: u64,
    /// `None` where the platform refuses one, which is a value like any other:
    /// it has to be *stable*, not present.
    modified: Option<SystemTime>,
}

/// Every entry under `root`, keyed by its path relative to it.
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
        None,
    );
    // **The watch is read here and torn down by going out of scope**, which is what
    // puts its teardown inside the window: `drive` returns before the caller takes
    // the second reading. Calling `watcher.stopper().stop()` first does **not**
    // order the teardown, whatever a comment beside it says.
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
        leanest_frame: if rig.frames == 0 {
            0
        } else {
            rig.leanest_frame
        },
        warmed: warmer.join().expect("the warmer finished").warmed,
        events: watcher.delivered(),
    }
}

/// What the drive actually did, for the caller to check before it checks the
/// filesystem.
struct Driven {
    /// Frames painted. Eight: a first paint, six actions and the second tick.
    frames: usize,
    /// The tallest body any of them drew, so a blank pane cannot pass as a run.
    body_rows: usize,
    /// Rows of **content**, summed across the whole run.
    content_rows: u64,
    /// Content rows in the **leanest** frame, which is the statistic that bites.
    leanest_frame: u64,
    /// Files the warmer compiled, which is the one stage that spawns a thread.
    warmed: usize,
    /// Raw events the armed watch was handed, **reported and not gated.**
    events: u64,
}

/// The shell's parts, held together so one can be borrowed while another paints.
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
    fn paint(&mut self) -> usize {
        let chrome = self.app.chrome("fixture", None, Pointing::default(), 0, "");
        let body = body_layout(
            area(),
            &chrome,
            self.frame.files().len(),
            self.frame.files().len(),
        );
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
    let linked = made_link(&scratch, "src/mod_0.rs", "link_to_mod_0.rs");

    // The fixture's own git writes must have landed before the window opens,
    // or the tail of the commit lands inside it and reads as a write by the
    // monitor.
    settle_tree(&root);
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

    // The fixture's own git writes must have landed before the window opens,
    // or the tail of the commit lands inside it and reads as a write by the
    // monitor.
    settle_tree(&root);
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
