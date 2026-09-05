//! `SPEC.md` §11.2 B21: a note anchors to a diff line and lives in a
//! per-worktree store.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use support::Scratch;
use vigia_core::{
    Error, NEAR, Note, Placement, Side, Status, Store, Worktree, key, resolve, state_root,
};

/// A directory the test owns, removed on drop, for a state root that is not
/// the reader's.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "vigia-notes-{}-{}-{name}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create a temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn note(id: &str, line: u32, text: &str, body: &str) -> Note {
    Note {
        id: id.to_owned(),
        path: "src/watch.rs".to_owned(),
        side: Side::New,
        line,
        text: text.to_owned(),
        body: body.to_owned(),
        status: Status::Open,
        reply: None,
        written: UNIX_EPOCH + Duration::from_secs(1_800_000_000),
    }
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read the store directory")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn the_key_is_one_value_for_one_worktree_from_any_path_inside_it() {
    // Two processes start from two paths: the pane from wherever it was
    // launched, the server from the project root. Both discover the same
    // workdir and must land on one directory.
    let scratch = Scratch::new("notes-key");
    scratch.write("src/a.rs", "fn a() {}\n");
    scratch.commit_all("first");

    let from_root = key(Worktree::discover(scratch.root())
        .expect("discover")
        .workdir())
    .expect("a key from the root");
    let from_inside = key(Worktree::discover(scratch.path_of("src"))
        .expect("discover from inside")
        .workdir())
    .expect("a key from inside");
    assert_eq!(from_root, from_inside);
    assert_eq!(from_root.len(), 40, "forty hex characters: {from_root}");
    assert!(from_root.chars().all(|c| c.is_ascii_hexdigit()));

    let other = Scratch::new("notes-key-other");
    assert_ne!(from_root, key(other.root()).expect("another key"));
}

#[test]
fn a_linked_worktree_has_its_own_key() {
    // Its diff is its own, so its store is too: the workdir gix answers with
    // is the linked root, not the main tree's.
    let scratch = Scratch::new("notes-key-linked");
    scratch.write("a.txt", "one\n");
    scratch.commit_all("first");
    let holder = TempDir::new("linked");
    let linked_root = holder.path().join("tree");
    scratch.git(&[
        "worktree",
        "add",
        "-q",
        linked_root.to_str().expect("a UTF-8 temp path"),
        "-b",
        "linked",
    ]);

    let linked = Worktree::discover(&linked_root).expect("discover the linked worktree");
    assert_ne!(
        key(scratch.root()).expect("the main key"),
        key(linked.workdir()).expect("the linked key")
    );
}

#[test]
fn a_note_put_by_one_handle_is_read_whole_by_another() {
    // The pane quitting and coming back, or the server reading what the pane
    // wrote: a second handle on the same worktree sees every field.
    let scratch = Scratch::new("notes-roundtrip");
    let root = TempDir::new("state");
    let store = Store::open(root.path(), scratch.root()).expect("open");
    let mut written = note(
        "a1",
        5,
        "    margin.checked_mul(2).unwrap_or(margin)",
        "use saturating_mul\nand drop the unwrap_or.\n\nreply 3\nbody 0",
    );
    written.reply = Some("swapped for saturating_mul; the unwrap_or went with it".to_owned());
    written.status = Status::Resolved;
    written.side = Side::Old;
    store.put(&written).expect("put");

    let again = Store::open(root.path(), scratch.root()).expect("open again");
    let listing = again.list().expect("list");
    assert_eq!(listing.notes, vec![written]);
    assert!(listing.skipped.is_empty(), "{:?}", listing.skipped);
}

#[test]
fn the_store_creates_nothing_until_the_first_put_and_one_put_is_one_file() {
    let scratch = Scratch::new("notes-one-file");
    let root = TempDir::new("state");
    let store = Store::open(root.path(), scratch.root()).expect("open");
    assert!(
        !store.dir().exists(),
        "opening the store created its directory"
    );
    assert!(store.list().expect("list an absent store").notes.is_empty());

    store.put(&note("a1", 1, "x", "first")).expect("put");
    assert_eq!(files_in(store.dir()), vec!["a1.note".to_owned()]);

    // A status change rewrites the same file rather than adding one.
    store.put(&note("a1", 1, "x", "second")).expect("rewrite");
    assert_eq!(files_in(store.dir()), vec!["a1.note".to_owned()]);
    assert_eq!(store.list().expect("list").notes[0].body, "second");
}

#[test]
fn removing_a_note_twice_is_not_an_error() {
    // Two panes on one worktree may both withdraw it.
    let scratch = Scratch::new("notes-remove");
    let root = TempDir::new("state");
    let store = Store::open(root.path(), scratch.root()).expect("open");
    store.put(&note("a1", 1, "x", "y")).expect("put");
    store.remove("a1").expect("remove");
    assert!(files_in(store.dir()).is_empty());
    store.remove("a1").expect("a second remove is a no-op");
    store
        .remove("never")
        .expect("removing what was never there is a no-op");
}

#[test]
fn a_torn_file_and_a_newer_version_are_skipped_and_the_rest_listed() {
    let scratch = Scratch::new("notes-skipped");
    let root = TempDir::new("state");
    let store = Store::open(root.path(), scratch.root()).expect("open");
    let good = note("good", 2, "fine", "kept");
    store.put(&good).expect("put");
    // A write cut off by a kill: the body announces more bytes than follow.
    fs::write(
        store.dir().join("torn.note"),
        "vigia note 1\nid: torn\npath: p\nside: new\nline: 1\nstatus: open\nwritten: 1\ntext: x\nbody 40\nshort\n",
    )
    .expect("write a torn file");
    fs::write(
        store.dir().join("newer.note"),
        "vigia note 2\nid: newer\nseverity: high\n",
    )
    .expect("write a newer file");
    // A write in flight from another process is neither listed nor reported.
    fs::write(store.dir().join("inflight.1f.tmp"), "vigia note 1\nid: in").expect("write a tmp");

    let listing = store.list().expect("list");
    assert_eq!(listing.notes, vec![good]);
    let mut skipped: Vec<(String, String)> = listing
        .skipped
        .iter()
        .map(|(path, why)| {
            (
                path.file_name()
                    .expect("a file name")
                    .to_string_lossy()
                    .into_owned(),
                why.clone(),
            )
        })
        .collect();
    skipped.sort();
    assert_eq!(skipped.len(), 2, "{skipped:?}");
    assert_eq!(skipped[0].0, "newer.note");
    assert!(skipped[0].1.contains("vigia note 2"), "{}", skipped[0].1);
    assert_eq!(skipped[1].0, "torn.note");
    assert!(skipped[1].1.contains("shorter"), "{}", skipped[1].1);
}

#[test]
fn an_unwritable_root_is_an_error_the_caller_can_show() {
    // B7's rule: a monitor that dies of its own writes is worse than none, so
    // the failure is a value the footer can say, not a panic.
    let scratch = Scratch::new("notes-unwritable");
    let root = TempDir::new("state");
    let file = root.path().join("a-file");
    fs::write(&file, "not a directory").expect("write a file where a root would be");
    let store = Store::open(&file, scratch.root()).expect("opening needs no directory");

    let err = store
        .put(&note("a1", 1, "x", "y"))
        .expect_err("a file is not a root");
    assert!(matches!(err, Error::Store { .. }), "{err:?}");
    assert!(!err.to_string().is_empty());
    assert!(!store.dir().exists());
}

#[test]
fn a_listing_sees_a_whole_note_while_it_is_being_rewritten() {
    // Temp-and-rename is what makes a mid-write listing safe; a torn read
    // would surface as a skipped file or a body that is neither version.
    let scratch = Scratch::new("notes-atomic");
    let root = TempDir::new("state");
    let store = Store::open(root.path(), scratch.root()).expect("open");
    let a = note("w", 3, "line", &"A".repeat(8192));
    let b = note("w", 3, "line", &"B".repeat(8192));
    store.put(&a).expect("first put");

    let writer_store = store.clone();
    let (wa, wb) = (a.clone(), b.clone());
    let writer = std::thread::spawn(move || {
        for round in 0..200 {
            let next = if round % 2 == 0 { &wb } else { &wa };
            writer_store.put(next).expect("rewrite under a reader");
        }
    });
    let mut listed = 0;
    while !writer.is_finished() {
        let listing = store.list().expect("list under a writer");
        assert!(
            listing.skipped.is_empty(),
            "a listing saw a torn note: {:?}",
            listing.skipped
        );
        for found in &listing.notes {
            assert!(
                found == &a || found == &b,
                "a listing saw a note that is neither version whole"
            );
            listed += 1;
        }
    }
    writer.join().expect("the writer finished");
    assert!(
        listed > 0,
        "the reader never listed anything while the writer ran"
    );
    assert_eq!(
        files_in(store.dir()),
        vec!["w.note".to_owned()],
        "a temporary was left behind"
    );
}

#[test]
fn a_line_is_found_where_it_was_then_by_its_text_nearby_then_judged_changed_or_gone() {
    let pinned = note("r", 5, "b", "");
    // Where it was.
    assert_eq!(
        resolve(&pinned, &[(4, "a"), (5, "b"), (6, "c")]),
        Placement::At(5)
    );
    // An edit above pushed it down one.
    assert_eq!(
        resolve(&pinned, &[(4, "a"), (5, "inserted"), (6, "b"), (7, "c")]),
        Placement::Moved(6)
    );
    // The line itself was edited, which is what the agent doing the note asks
    // for looks like.
    assert_eq!(
        resolve(&pinned, &[(4, "a"), (5, "b2"), (6, "c")]),
        Placement::Changed
    );
    // Neither the number nor the text is drawn.
    assert_eq!(resolve(&pinned, &[(1, "a"), (2, "c")]), Placement::Gone);
    // Its text too far away does not count as a move.
    let far = 5 + NEAR + 1;
    assert_eq!(
        resolve(&pinned, &[(5, "x"), (far, "b")]),
        Placement::Changed
    );
    assert_eq!(resolve(&pinned, &[(far, "b")]), Placement::Gone);
    // The nearest copy wins, and the earlier one on a tie, in either order.
    assert_eq!(resolve(&pinned, &[(2, "b"), (6, "b")]), Placement::Moved(6));
    assert_eq!(resolve(&pinned, &[(3, "b"), (7, "b")]), Placement::Moved(3));
    assert_eq!(resolve(&pinned, &[(7, "b"), (3, "b")]), Placement::Moved(3));
    // An empty line can carry a note, and an empty line is found by its number.
    let blank = note("e", 9, "", "");
    assert_eq!(
        resolve(&blank, &[(8, "x"), (9, ""), (10, "")]),
        Placement::At(9)
    );
}

#[test]
fn the_state_root_follows_xdg_then_home_and_localappdata_on_windows() {
    let env = |vars: &[(&str, &str)]| {
        let owned: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name: &str| {
            owned
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    };
    // Absolute on every platform, which is what the XDG rule turns on.
    let absolute = std::env::temp_dir().join("xdg-state");
    let absolute_str = absolute.to_str().expect("a UTF-8 temp dir");

    assert_eq!(
        state_root(
            false,
            &env(&[("XDG_STATE_HOME", absolute_str), ("HOME", "/home/r")])
        ),
        Some(absolute.join("vigia"))
    );
    // A relative value is invalid and ignored, per the specification.
    assert_eq!(
        state_root(
            false,
            &env(&[("XDG_STATE_HOME", "state"), ("HOME", "/home/r")])
        ),
        Some(PathBuf::from("/home/r/.local/state/vigia"))
    );
    // Set but empty is unset.
    assert_eq!(
        state_root(
            false,
            &env(&[("XDG_STATE_HOME", "  "), ("HOME", "/home/r")])
        ),
        Some(PathBuf::from("/home/r/.local/state/vigia"))
    );
    assert_eq!(
        state_root(false, &env(&[("USERPROFILE", "/u/r")])),
        Some(PathBuf::from("/u/r/.local/state/vigia"))
    );
    assert_eq!(state_root(false, &env(&[])), None);
    assert_eq!(
        state_root(
            true,
            &env(&[
                ("LOCALAPPDATA", "C:\\Users\\r\\AppData\\Local"),
                ("HOME", "/h")
            ])
        ),
        Some(PathBuf::from("C:\\Users\\r\\AppData\\Local\\vigia\\state"))
    );
    assert_eq!(state_root(true, &env(&[("HOME", "/h")])), None);
}

#[cfg(windows)]
#[test]
fn two_spellings_of_one_path_and_a_junction_derive_one_key() {
    // The filesystem is case-insensitive and a junction is a second name, so
    // both are one worktree and must be one store.
    let scratch = Scratch::new("notes-key-spelling");
    let root = scratch.root();
    let spelled = root.to_string_lossy().into_owned();
    let last = spelled
        .rsplit(['\\', '/'])
        .next()
        .expect("a last component")
        .to_owned();
    let upper = PathBuf::from(format!(
        "{}{}",
        &spelled[..spelled.len() - last.len()],
        last.to_uppercase()
    ));
    assert_ne!(
        upper, root,
        "the fixture name has no letter to change case on"
    );
    assert_eq!(
        key(root).expect("key"),
        key(&upper).expect("key of the other spelling")
    );

    let holder = TempDir::new("junction");
    let link = holder.path().join("link");
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(root)
        .output()
        .expect("run mklink");
    assert!(
        made.status.success(),
        "mklink: {}",
        String::from_utf8_lossy(&made.stderr)
    );
    assert_eq!(
        key(root).expect("key"),
        key(&link).expect("key through the junction")
    );
}
