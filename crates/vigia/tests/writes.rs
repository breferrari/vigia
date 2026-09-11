//! `SPEC.md` §11.1: the monitor writes nothing.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::post::{content, post_each};
use vigia::{
    Action, App, Committed, Config, Glyphs, Input, Key, PaintStats, Pointing, Theme, body_layout,
    commit, config, opening, regions, render, state_root,
};
use vigia_core::{
    Frame, Highlighter, History, Note, Registry, Standing, Store, WARM_FILES, WatchOptions,
    Worktree,
};

use support::{Scratch, TempDir, made_link, note, registration, settle_tree};

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
    /// Directories are stamped too, which is what catches a file created and
    /// deleted inside one window: adding or removing a child moves the parent's
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
    // The root is stamped as well, and it was not until the mutation run said so.
    found.insert(PathBuf::from("."), stamp_of(root));
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir).expect("read a fixture directory");
        for entry in entries {
            let entry = entry.expect("read a fixture directory entry");
            let path = entry.path();
            let stamp = stamp_of(&path);
            // `rela` first so `path` can be *moved* into the queue rather than cloned
            // into it.
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

/// One entry's stamp, from a fresh query rather than a cached one.
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

    // A real watch, armed for the length of the run.
    let watcher = worktree
        .watch(WatchOptions::default())
        .expect("arm a real watch");

    // The height comes from the frame before each action, not from the first one.
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

    // Joined rather than detached, which is the opposite of what `run` does and is
    // right here.
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
    // The watch is read here and torn down by going out of scope, which is what puts
    // its teardown inside the window: `drive` returns before the caller takes the
    // second reading.
    Driven {
        frames: rig.frames,
        body_rows: rig.body_rows,
        content_rows: rig.painted.rows,
        // Collapsed to zero when no frame ran, so the guard rests on its own
        // evidence. The running minimum starts at `u64::MAX` and a `drive` that
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
    /// Rows of content, summed across the whole run.
    content_rows: u64,
    /// Content rows in the leanest frame, which is the statistic that bites.
    leanest_frame: u64,
    /// Files the warmer compiled, which is the one stage that spawns a thread.
    warmed: usize,
    /// Raw events the armed watch was handed, reported and not gated.
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
        let chrome = self.app.chrome(
            "fixture",
            None,
            vigia::Stood {
                standing: &Standing::Current,
                now: 0,
            },
            Pointing::default(),
            Default::default(),
            "",
        );
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

    // A symlink, because [`Stamp`] argues about links at length and no fixture here had
    // one. §7's rule is that an axis named as unspanned is a prediction, and this file
    // was making the prediction in a doc comment: the reason given for
    // `symlink_metadata` over `metadata` could not fail, because nothing under the root
    // was a link.
    let linked = made_link(&scratch, "src/mod_0.rs", "link_to_mod_0.rs");

    // The fixture's own git writes must have landed before the window opens,
    // or the tail of the commit lands inside it and reads as a write by the
    // monitor.
    settle_tree(&root);
    let before = snapshot(&root);
    // The state directory B21 opens is outside the tree, so the tree's snapshot
    // cannot see a write there; it gets its own, at the root the shell would
    // resolve from this process's environment.
    let state = state_root(cfg!(windows), |name| std::env::var(name).ok());
    let state_before = state.as_deref().filter(|dir| dir.exists()).map(snapshot);
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

    // The store's own watch, armed for the run on a root of its own, so its
    // footprint sits inside a window this gate compares: `Store::watch` creates
    // nothing, and this is where that claim is checked rather than trusted.
    let watched = TempDir::new("writes-nothing-watch");
    let store = Store::open(watched.path(), &root).expect("open the store");
    let watch = store
        .watch(|| {})
        .expect("arm the store watch")
        .expect("the root exists, so there is something to arm on");
    settle_tree(watched.path());
    let watched_before = snapshot(watched.path());

    let driven = drive(&root);
    drop(watch);
    let watched_moved = difference(&watched_before, &snapshot(watched.path()));
    assert!(
        watched_moved.is_empty(),
        "SPEC.md §11.1: the monitor writes nothing of its own, and the store's \
         watch moved {} entries under the root it was armed on:\n{}",
        watched_moved.len(),
        watched_moved.join("\n")
    );

    // Before the filesystem is compared, not after.
    assert_eq!(
        driven.frames, 8,
        "the drive painted {} frames rather than the first paint, six actions and \
         the second tick, so the window this gate compares is not the one it \
         describes",
        driven.frames
    );
    // A heading is pushed per changed file before any hunk is reached, so a body with
    // rows in it proves only that the fixture had files.
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
    if let Some(dir) = state.as_deref() {
        let state_after = dir.exists().then(|| snapshot(dir));
        let moved = match (&state_before, &state_after) {
            (None, None) => Vec::new(),
            (Some(before), Some(after)) => difference(before, after),
            (None, Some(_)) => vec![format!("{} was created", dir.display())],
            (Some(_), None) => vec![format!("{} was removed", dir.display())],
        };
        assert!(
            moved.is_empty(),
            "SPEC.md §11.1: the monitor writes nothing of its own, and with no \
             gesture this run moved {} entries under the state directory:\n{}",
            moved.len(),
            moved.join("\n")
        );
    }
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

    // A different length, deliberately.
    scratch.write("src/mod_0.rs", "one line\n".repeat(LINES * 2));
    scratch.write("untracked.txt", "and a file that was not there before\n");
    // The third direction, and it had no case here until round 3 of the audit said so.
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

#[test]
fn one_gesture_writes_exactly_one_file() {
    // The other half of §11.1's amended rule: what the reader asked for goes to
    // the state directory, once, and nothing rides along with it.
    let scratch = Scratch::large_diff("writes-one-gesture", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let root = TempDir::new("writes-one-gesture-state");
    let store = Store::open(root.path(), scratch.root()).expect("open the store");

    // A painted frame, so a row's gutter exists to be pressed.
    let mut app = App::past_first_paint();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let chrome = app.chrome(
        "fixture",
        None,
        vigia::Stood {
            standing: &Standing::Current,
            now: 0,
        },
        Pointing::default(),
        Default::default(),
        "",
    );
    let body = body_layout(area(), &chrome, frame.files().len(), frame.files().len());
    let view = app
        .view(&mut frame, &mut highlighter, &history, body)
        .expect("collect a view");
    let laid = regions(area(), &chrome, &view);
    let line = view
        .rows
        .iter()
        .position(|row| matches!(row, vigia::Row::Line { .. }))
        .expect("a content row on the first screen");
    let (left, _) = laid.diff.gutter;
    assert!(
        laid.gutter_at(left, laid.diff.top + line as u16).is_some(),
        "the row this gate presses has no gutter"
    );

    settle_tree(root.path());
    let before = snapshot(root.path());

    // The press opens the box and writes nothing: the gesture is Enter.
    let (anchor, existing) = opening(&view, line, app.notes()).expect("the press opened nothing");
    app.open_box(anchor, existing.as_ref());
    for c in "use saturating_mul".chars() {
        app.box_edit(Input {
            key: Key::Char(c),
            ctrl: false,
            alt: false,
            shift: false,
        });
    }
    assert!(
        difference(&before, &snapshot(root.path())).is_empty(),
        "opening the box and typing into it wrote to the state root before Enter"
    );

    let written = match commit(&store, app.note_box().expect("the box is open")) {
        Ok(Committed::Written(id)) => id,
        other => panic!("Enter did not write a note: {other:?}"),
    };
    let moved = difference(&before, &snapshot(root.path()));

    // The root's own stamp moves because the store's directory appeared in it,
    // the directory appeared, and one file appeared inside it. Nothing else.
    let created: Vec<&String> = moved
        .iter()
        .filter(|line| line.ends_with("was created"))
        .collect();
    assert_eq!(
        created.len(),
        2,
        "one gesture created {} entries under the state root rather than the \
         store's directory and one note file:\n{}",
        created.len(),
        moved.join("\n")
    );
    assert!(
        created
            .iter()
            .any(|line| line.contains(&format!("{written}.note"))),
        "the one file created is not the note: {moved:?}"
    );
    assert!(
        moved
            .iter()
            .all(|line| line.ends_with("was created") || line.starts_with(". moved")),
        "something other than the root's own stamp changed under the state root:\n{}",
        moved.join("\n")
    );
}

/// §11.2 B21's send rung reads the registry and never writes it: the hook owns
/// that file, and a pane that rewrote it would be the monitor writing something
/// the reader did not type.
#[test]
fn a_registered_session_is_read_and_never_written() {
    let scratch = Scratch::large_diff("writes-registered", FILES, LINES);
    let root = TempDir::new("writes-registered-state");
    let store = Store::open(root.path(), scratch.root()).expect("open the store");
    let registry = Registry::open(root.path(), scratch.root()).expect("open the registry");

    let registration = registration("aaaa-1111", "inbox.sock");
    registry.put(&registration).expect("register");

    let note = Note {
        path: "src/a.rs".to_owned(),
        ..note("n1", 1, "one", "use saturating_mul")
    };

    settle_tree(root.path());
    let before = snapshot(root.path());

    // The whole of Enter's second half, over a transport that takes everything.
    store.put(&note).expect("the store took it");
    let posted = post_each(&registry, || content(&note, &[]), |_, _| Ok(()));
    assert_eq!(posted, vigia::Posted::Sent);

    let moved = difference(&before, &snapshot(root.path()));
    assert!(
        moved.iter().all(|line| !line.contains(".session")),
        "the gesture touched a registration the hook owns:\n{}",
        moved.join("\n")
    );
    let created: Vec<&String> = moved
        .iter()
        .filter(|line| line.ends_with("was created"))
        .collect();
    // The store's directory and the note inside it, as the sibling above counts
    // them. The registry's directory is already there and stays untouched.
    assert_eq!(
        created.len(),
        2,
        "one gesture created {} entries rather than the store's directory and one \
         note file:\n{}",
        created.len(),
        moved.join("\n")
    );
    assert!(
        created.iter().any(|line| line.contains("n1.note")),
        "no note file was created: {moved:?}"
    );
    assert_eq!(
        registry.list().expect("still registered"),
        vec![registration],
        "the registration did not survive the gesture unchanged"
    );
}

#[test]
fn a_flip_with_remembering_off_writes_nothing_at_all() {
    // The half B22 turns on: remembering is opt-in, so the shipped pane writes
    // nothing whatever the reader presses. The gate is over the file rather than
    // over the flag, because a flag read the wrong way round still passes a test
    // that asks the flag.
    let home = Scratch::new("persist-off-home");
    let path = home.root().join(".config/vigia/config");
    let mut app = App::new();
    assert!(!app.settings().persist, "the shipped pane remembers");

    let mut frame_scratch = Scratch::large_diff("persist-off", FILES, LINES);
    let worktree = frame_scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    for action in [Action::ToggleRail, Action::ToggleWrap] {
        app.apply(action, &mut frame, 0).expect("flip");
        if app.settings().persist {
            config::save(&path, &app.config()).expect("save");
        }
    }
    assert!(
        !path.exists(),
        "a flip with remembering off wrote {}",
        path.display()
    );

    // And the arm the loop skipped is one a reader reaches: with the row turned on
    // the same joint puts the file there, so what is asserted above is a write that
    // was live and held back rather than a call nothing could have made.
    app.apply(Action::TogglePersist, &mut frame, 0)
        .expect("turn remembering on");
    assert!(
        app.settings().persist,
        "the row did not turn remembering on"
    );
    config::save(&path, &app.config()).expect("save");
    assert!(
        path.exists(),
        "the same save with remembering on wrote nothing"
    );
    let _ = &mut frame_scratch;
}

#[test]
fn remembering_writes_what_the_reader_flipped_and_nothing_else() {
    let home = Scratch::new("persist-on-home");
    let path = home.root().join(".config/vigia/config");
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the directory");
    std::fs::write(&path, "# mine\nhide = ^target/\nrail = off\n").expect("seed the file");

    let mut config = config::load(&path).expect("the seed parses");
    config.persist = true;
    let mut app = App::configured(&config);

    let mut frame_scratch = Scratch::large_diff("persist-on", FILES, LINES);
    let worktree = frame_scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    app.apply(Action::ToggleRail, &mut frame, 0).expect("flip");
    config::save(
        &path,
        &Config {
            hide: config.hide.clone(),
            ..app.config()
        },
    )
    .expect("save");

    let written = std::fs::read_to_string(&path).expect("read it back");
    assert!(
        written.contains("# mine"),
        "the comment is gone:\n{written}"
    );
    assert!(
        written.contains("hide = ^target/"),
        "the pattern is gone:\n{written}"
    );
    assert!(
        written.contains("rail = on"),
        "the flip did not land:\n{written}"
    );
    // And exactly one file, with no temp left beside it.
    let beside: Vec<String> = std::fs::read_dir(path.parent().expect("a parent"))
        .expect("read the directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        beside,
        vec!["config".to_owned()],
        "the save left a file behind"
    );
    let _ = &mut frame_scratch;
}
