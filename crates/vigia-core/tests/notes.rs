//! `SPEC.md` §11.2 B21: a note anchors to a diff line and lives in a
//! per-worktree store.

mod support;

use std::fs;
use std::sync::mpsc;
use std::time::{Duration, UNIX_EPOCH};

use support::{Scratch, TempDir, budget, files_in, note};
use vigia_core::{
    Error, FileDiff, Hunk, Line, LineKind, NEAR, Note, Placement, Side, Status, Store, Worktree,
    key, resolve,
};

/// A store on a fresh state root for a fresh repository.
fn store(name: &str) -> (Scratch, TempDir, Store) {
    let scratch = Scratch::new(name);
    let root = TempDir::new("state");
    let store = Store::open(root.path(), scratch.root()).expect("open");
    (scratch, root, store)
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
fn a_workdir_that_does_not_exist_has_no_key() {
    let holder = TempDir::new("missing");
    let missing = holder.path().join("never-made");
    let err = key(&missing).expect_err("no key for a path that is not there");
    assert!(matches!(err, Error::Canonicalise { .. }), "{err:?}");
    assert!(err.to_string().contains("never-made"), "{err}");
    assert!(
        Store::open(holder.path(), &missing).is_err(),
        "a store opened on a missing worktree would key nothing"
    );
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

#[cfg(unix)]
#[test]
fn a_symlinked_worktree_root_derives_the_same_key() {
    // A second name for one directory is one worktree, and must be one store.
    let scratch = Scratch::new("notes-key-symlink");
    let holder = TempDir::new("symlink");
    let link = holder.path().join("link");
    std::os::unix::fs::symlink(scratch.root(), &link).expect("make a symlink");
    assert_eq!(
        key(scratch.root()).expect("key"),
        key(&link).expect("key through the symlink")
    );
}

#[test]
fn every_field_of_a_note_is_read_back_whole_by_a_second_handle() {
    // The pane quitting and coming back, or the server reading what the pane
    // wrote: a second handle on the same worktree sees every field, at every
    // status and on either side, with a body that holds the format's own words.
    let (scratch, root, store) = store("notes-roundtrip");
    let mut notes = Vec::new();
    for (i, (status, side)) in [
        (Status::Open, Side::New),
        (Status::Seen, Side::Old),
        (Status::Resolved, Side::New),
    ]
    .into_iter()
    .enumerate()
    {
        let mut written = note(
            &format!("n{i}"),
            5,
            "    margin.checked_mul(2).unwrap_or(margin)",
            "use saturating_mul\nand drop the unwrap_or.\n\nreply 3\nbody 0\npath 1",
        );
        written.reply = Some(format!("swapped for saturating_mul {i}"));
        written.status = status;
        written.side = side;
        written.written = UNIX_EPOCH + Duration::from_secs(1_800_000_000 + i as u64);
        store.put(&written).expect("put");
        notes.push(written);
    }

    let again = Store::open(root.path(), scratch.root()).expect("open again");
    let listing = again.list().expect("list");
    assert_eq!(listing.notes, notes);
    assert!(listing.skipped.is_empty(), "{:?}", listing.skipped);
}

#[test]
fn a_path_or_a_line_with_a_newline_in_it_cannot_forge_a_field() {
    // A file name may hold a newline on Unix, and a line of code may hold a
    // carriage return; neither may become a header the decoder believes.
    let (_scratch, _root, store) = store("notes-forge");
    let mut written = note("f1", 1, "a\rb", "body");
    written.path = "x\nid: stolen\nbody 0".to_owned();
    store.put(&written).expect("put");

    let listing = store.list().expect("list");
    assert_eq!(listing.notes, vec![written]);
    assert!(listing.skipped.is_empty(), "{:?}", listing.skipped);
}

#[test]
fn the_store_creates_nothing_until_the_first_put() {
    let (_scratch, _root, store) = store("notes-lazy");
    assert!(
        !store.dir().exists(),
        "opening the store created its directory"
    );
    assert!(store.list().expect("list an absent store").notes.is_empty());

    store.put(&note("a1", 1, "x", "first")).expect("put");
    assert_eq!(files_in(store.dir()), vec!["a1.note".to_owned()]);
}

#[test]
fn a_rewrite_reuses_the_note_file() {
    // A status change rewrites the same file rather than adding one.
    let (_scratch, _root, store) = store("notes-rewrite");
    store.put(&note("a1", 1, "x", "first")).expect("put");
    store.put(&note("a1", 1, "x", "second")).expect("rewrite");
    assert_eq!(files_in(store.dir()), vec!["a1.note".to_owned()]);
    assert_eq!(store.list().expect("list").notes[0].body, "second");
}

#[test]
fn removing_a_note_twice_is_not_an_error() {
    // Two panes on one worktree may both withdraw it.
    let (_scratch, _root, store) = store("notes-remove");
    store.put(&note("a1", 1, "x", "y")).expect("put");
    store.remove("a1").expect("remove");
    assert!(files_in(store.dir()).is_empty());
    store.remove("a1").expect("a second remove is a no-op");
    store
        .remove("never")
        .expect("removing what was never there is a no-op");
}

#[test]
fn an_id_that_could_name_a_file_outside_the_store_is_refused() {
    // An id here may be one an agent typed, handed through the server.
    let (_scratch, _root, store) = store("notes-id");
    let long = "x".repeat(65);
    for bad in [
        "../../evil",
        "a/b",
        "a\\b",
        "",
        ".",
        "..",
        "a b",
        "a\0b",
        "CON",
        "nul",
        "COM1",
        "lpt9",
        "AB1",
        "Ab-1",
        long.as_str(),
    ] {
        let err = store
            .put(&note(bad, 1, "x", "y"))
            .expect_err("an id that is not a note id");
        assert!(matches!(err, Error::Store { .. }), "{bad:?}: {err:?}");
        assert!(err.to_string().contains("not a note id"), "{bad:?}: {err}");
        assert!(store.remove(bad).is_err(), "{bad:?} was accepted by remove");
    }
    assert!(!store.dir().exists(), "a refused put created the directory");
    store
        .put(&note("ok-1", 1, "x", "y"))
        .expect("a plain id is fine");
    let longest = "y".repeat(64);
    store
        .put(&note(&longest, 1, "x", "y"))
        .expect("sixty-four is the longest id");
    store
        .put(&note("com0", 1, "x", "y"))
        .expect("com0 names no device");
}

#[test]
fn a_minted_id_is_one_the_store_accepts() {
    let (_scratch, _root, store) = store("notes-minted");
    let first = Store::new_id();
    let second = Store::new_id();
    assert_ne!(first, second, "two ids minted in a row must differ");
    store
        .put(&note(&first, 1, "x", "y"))
        .expect("a minted id is a note id");
    store
        .put(&note(&second, 2, "x", "y"))
        .expect("and so is the next");
    assert_eq!(store.list().expect("list").notes.len(), 2);
}

#[test]
fn a_file_whose_id_is_not_its_name_is_skipped() {
    // A rogue file naming another note's id would point a later removal at
    // the wrong file and survive every listing itself.
    let (_scratch, _root, store) = store("notes-rogue");
    let real = note("real", 1, "x", "kept");
    store.put(&real).expect("put");
    let encoded = fs::read(store.dir().join("real.note")).expect("read the note back");
    fs::write(store.dir().join("rogue.note"), &encoded).expect("write a rogue copy");

    let listing = store.list().expect("list");
    assert_eq!(listing.notes, vec![real]);
    assert_eq!(listing.skipped.len(), 1, "{:?}", listing.skipped);
    assert!(
        listing.skipped[0].1.contains("not its file name"),
        "{}",
        listing.skipped[0].1
    );
}

/// The note `good` as the store wrote it, and its file's text.
fn good_note(store: &Store) -> (Note, String) {
    let mut good = note("good", 1, "x", "kept");
    good.reply = Some("done".to_owned());
    store.put(&good).expect("put");
    let text = fs::read_to_string(store.dir().join("good.note")).expect("read it back");
    (good, text)
}

#[test]
fn a_repeated_field_is_a_skip() {
    let (_scratch, _root, store) = store("notes-repeat");
    let (good, text) = good_note(&store);
    let twice = text.replacen("side: new\n", "side: new\nside: old\n", 1);
    assert_ne!(twice, text);
    fs::write(store.dir().join("good.note"), &twice).expect("write a repeated field");

    let listing = store.list().expect("list");
    assert!(listing.notes.is_empty(), "{:?}", listing.notes);
    assert_eq!(listing.skipped.len(), 1);
    assert!(
        listing.skipped[0].1.contains("twice"),
        "{}",
        listing.skipped[0].1
    );
    assert_ne!(listing.skipped[0].1, good.body);
}

#[test]
fn trailing_bytes_are_a_skip() {
    let (_scratch, _root, store) = store("notes-trailing");
    let (_good, text) = good_note(&store);
    fs::write(store.dir().join("good.note"), format!("{text}extra\n"))
        .expect("write trailing bytes");

    let listing = store.list().expect("list");
    assert!(listing.notes.is_empty(), "{:?}", listing.notes);
    assert_eq!(listing.skipped.len(), 1);
    assert!(
        listing.skipped[0].1.contains("follow"),
        "{}",
        listing.skipped[0].1
    );
}

#[test]
fn a_note_read_back_in_another_field_order_is_the_same_note() {
    let (_scratch, _root, store) = store("notes-reordered");
    let (good, text) = good_note(&store);
    let reordered = text.replacen("id: good\nside: new\n", "side: new\nid: good\n", 1);
    assert_ne!(reordered, text);
    fs::write(store.dir().join("good.note"), &reordered).expect("write reordered fields");

    let listing = store.list().expect("list");
    assert_eq!(listing.notes, vec![good]);
    assert!(listing.skipped.is_empty(), "{:?}", listing.skipped);
}

#[test]
fn a_file_whose_name_is_not_a_note_id_is_not_read() {
    // The same rule that keeps a device name out of put keeps it out of read.
    let (_scratch, _root, store) = store("notes-bad-name");
    let (good, _text) = good_note(&store);
    fs::write(store.dir().join("Bad Name.note"), "anything").expect("write a badly named file");

    let listing = store.list().expect("list");
    assert_eq!(listing.notes, vec![good]);
    assert_eq!(listing.skipped.len(), 1, "{:?}", listing.skipped);
    assert!(
        listing.skipped[0].1.contains("not named by a note id"),
        "{}",
        listing.skipped[0].1
    );
}

#[test]
fn a_torn_file_and_a_newer_version_are_skipped_and_the_rest_listed() {
    let (_scratch, _root, store) = store("notes-skipped");
    let good = note("good", 2, "fine", "kept");
    store.put(&good).expect("put");
    // A write cut off by a kill: the body announces more bytes than follow.
    fs::write(
        store.dir().join("torn.note"),
        "vigia note 1\nid: torn\nside: new\nline: 1\nstatus: open\nwritten: 1\npath 1\np\ntext 1\nx\nbody 40\nshort\n",
    )
    .expect("write a torn file");
    fs::write(
        store.dir().join("newer.note"),
        "vigia note 2\nid: newer\nseverity: high\n",
    )
    .expect("write a newer file");
    // A write in flight from another process is neither listed nor reported.
    fs::write(
        store.dir().join("inflight.1f-2.tmp"),
        "vigia note 1\nid: in",
    )
    .expect("write a tmp");

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
fn a_number_too_large_for_the_file_is_a_skip_and_never_a_panic() {
    // A corrupt note costs that note and never the process.
    let (_scratch, _root, store) = store("notes-huge");
    store.put(&note("good", 1, "x", "kept")).expect("put");
    let head = "vigia note 1\nid: h\nside: new\nline: 1\nstatus: open\n";
    let files = [
        (
            "body.note",
            format!(
                "{head}written: 1\npath 1\np\ntext 1\nx\nbody {}\n\n",
                usize::MAX
            ),
        ),
        (
            "reply.note",
            format!(
                "{head}written: 1\npath 1\np\ntext 1\nx\nbody 0\n\nreply {}\n\n",
                usize::MAX
            ),
        ),
        (
            "time.note",
            format!(
                "{head}written: {}\npath 1\np\ntext 1\nx\nbody 0\n\n",
                u64::MAX
            ),
        ),
        ("empty.note", String::new()),
        ("version-only.note", "vigia note 1\n".to_owned()),
    ];
    for (name, contents) in &files {
        fs::write(store.dir().join(name), contents).expect("write a hostile file");
    }

    let listing = store.list().expect("list survives every file");
    assert_eq!(listing.notes.len(), 1);
    assert_eq!(listing.skipped.len(), files.len(), "{:?}", listing.skipped);
    assert!(listing.skipped.iter().all(|(_, why)| !why.is_empty()));
    let version_only = listing
        .skipped
        .iter()
        .find(|(path, _)| path.ends_with("version-only.note"))
        .expect("the version-only file was skipped");
    assert!(version_only.1.contains("ends before"), "{}", version_only.1);
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
fn a_failed_rename_leaves_no_temporary_behind() {
    // A directory where the note's file would go makes the rename fail on
    // every platform.
    let (_scratch, _root, store) = store("notes-litter");
    store
        .put(&note("other", 1, "x", "y"))
        .expect("put creates the directory");
    fs::create_dir(store.dir().join("blocked.note")).expect("a directory in the way");

    let err = store
        .put(&note("blocked", 1, "x", "y"))
        .expect_err("a rename onto a directory fails");
    assert!(matches!(err, Error::Store { .. }), "{err:?}");
    assert_eq!(
        files_in(store.dir()),
        vec!["blocked.note".to_owned(), "other.note".to_owned()],
        "a temporary was left behind"
    );
}

/// Rewrites one note between two versions from `writers` threads while the
/// caller lists, and asserts every listing saw one version whole and both
/// versions were seen. The writers stop once the reader has seen both, or
/// after a bounded number of rounds, so a starved reader fails on what it saw
/// rather than on timing.
fn rewrite_under_a_reader(name: &str, writers: usize) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let (_scratch, _root, store) = store(name);
    let a = note("w", 3, "line", &"A".repeat(8192));
    let b = note("w", 3, "line", &"B".repeat(8192));
    store.put(&a).expect("first put");

    let seen_both = Arc::new(AtomicBool::new(false));
    let handles: Vec<_> = (0..writers)
        .map(|w| {
            let writer_store = store.clone();
            let (wa, wb) = (a.clone(), b.clone());
            let seen_both = Arc::clone(&seen_both);
            std::thread::spawn(move || {
                for round in 0..4000 {
                    if seen_both.load(Ordering::Relaxed) {
                        break;
                    }
                    let next = if (round + w) % 2 == 0 { &wb } else { &wa };
                    writer_store.put(next).expect("rewrite under a reader");
                }
            })
        })
        .collect();
    let (mut saw_a, mut saw_b) = (false, false);
    while handles.iter().any(|h| !h.is_finished()) {
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
            saw_a |= found == &a;
            saw_b |= found == &b;
        }
        if saw_a && saw_b {
            seen_both.store(true, Ordering::Relaxed);
        }
        std::thread::yield_now();
    }
    for handle in handles {
        handle.join().expect("a writer finished");
    }
    assert!(
        saw_a && saw_b,
        "the reader saw one version only, so nothing was rewritten under it"
    );
    assert_eq!(
        files_in(store.dir()),
        vec!["w.note".to_owned()],
        "a temporary was left behind"
    );
}

#[test]
fn a_listing_sees_a_whole_note_while_it_is_being_rewritten() {
    // Temp-and-rename is what makes a mid-write listing safe; a torn read
    // would surface as a skipped file or a body that is neither version.
    rewrite_under_a_reader("notes-atomic", 1);
}

#[test]
fn two_writers_in_one_process_never_produce_a_mixed_note() {
    // Threads share a process id, so the temporary's name needs more than it.
    rewrite_under_a_reader("notes-atomic-threads", 2);
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
    // An exact match wins over a nearer duplicate that came first.
    assert_eq!(resolve(&pinned, &[(4, "b"), (5, "b")]), Placement::At(5));
    // An empty line can carry a note, and an empty line is found by its number.
    let blank = note("e", 9, "", "");
    assert_eq!(
        resolve(&blank, &[(8, "x"), (9, ""), (10, "")]),
        Placement::At(9)
    );
}

#[test]
fn rows_on_numbers_each_side_the_way_the_gutter_does() {
    // The resolver takes the rows a side holds, numbered as the pane numbers
    // them: a removal by the index, everything else by the working tree, and a
    // context line on the working-tree side. A drift here is a note found one
    // row off from the line it was pinned to.
    let line = |kind, text: &str| Line {
        kind,
        text: text.to_owned(),
        emph: Vec::new(),
    };
    let diff = FileDiff {
        path: "src/watch.rs".to_owned(),
        binary: false,
        unreadable: None,
        hunks: vec![
            Hunk {
                old_start: 10,
                old_lines: 3,
                new_start: 12,
                new_lines: 3,
                lines: vec![
                    line(LineKind::Context, "a"),
                    line(LineKind::Removed, "b"),
                    line(LineKind::Added, "c"),
                    line(LineKind::Context, "d"),
                ],
            },
            Hunk {
                old_start: 40,
                old_lines: 1,
                new_start: 42,
                new_lines: 1,
                lines: vec![line(LineKind::Context, "e")],
            },
        ],
        added: 1,
        removed: 1,
        lines: 60,
        first_line: None,
        bytes: 0,
    };

    assert_eq!(
        diff.rows_on(Side::New),
        vec![(12, "a"), (13, "c"), (14, "d"), (42, "e")]
    );
    assert_eq!(diff.rows_on(Side::Old), vec![(11, "b")]);
    // The two sides together are every line, once each.
    assert_eq!(
        diff.rows_on(Side::New).len() + diff.rows_on(Side::Old).len(),
        diff.hunks
            .iter()
            .map(|hunk| hunk.lines.len())
            .sum::<usize>()
    );
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
    let upper = std::path::PathBuf::from(format!(
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

/// B21: the store is an event source for both processes. Armed on the state
/// root while the store's directory is not there yet, then on the directory.
#[test]
fn a_write_from_another_handle_wakes_the_store_watch() {
    let (scratch, root, store) = store("notes-watch");
    let other = Store::open(root.path(), scratch.root()).expect("a second handle");
    assert!(
        !store.dir().exists(),
        "the directory appears on the first put"
    );

    let (tx, rx) = mpsc::channel();
    let watch = store
        .watch(move || {
            let _ = tx.send(());
        })
        .expect("arm on the root")
        .expect("the root exists, so there is something to watch");
    other.put(&note("w-1", 5, "x", "first")).expect("put");
    rx.recv_timeout(budget(Duration::from_secs(5)))
        .expect("the first write, which also created the directory, was reported");
    drop(watch);

    let (tx, rx) = mpsc::channel();
    let watch = store
        .watch(move || {
            let _ = tx.send(());
        })
        .expect("arm on the directory")
        .expect("the directory exists now");
    other
        .put(&note("w-1", 5, "x", "rewritten"))
        .expect("rewrite");
    rx.recv_timeout(budget(Duration::from_secs(5)))
        .expect("a rewrite was reported");
    while rx.try_recv().is_ok() {}
    other.remove("w-1").expect("remove");
    rx.recv_timeout(budget(Duration::from_secs(5)))
        .expect("a removal was reported");
    drop(watch);
}

#[test]
fn a_store_whose_root_is_not_there_yet_has_nothing_to_watch() {
    let scratch = Scratch::new("notes-watch-nothing");
    let holder = TempDir::new("state");
    let store = Store::open(&holder.path().join("never-made"), scratch.root()).expect("open");
    let armed = store
        .watch(|| {})
        .expect("nothing to watch is not an error");
    assert!(armed.is_none(), "there is no directory on disk to arm on");
    assert!(
        !holder.path().join("never-made").exists(),
        "and none was made for it"
    );
}
