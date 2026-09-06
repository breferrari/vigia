//! `SPEC.md` §11.2 B21's send rung: where a running agent session records the
//! socket the pane posts a note into.

mod support;

use std::fs;
use std::time::{Duration, SystemTime};

use support::{Scratch, TempDir, files_in, registration};
use vigia_core::{Registry, Store};

/// A registry on a fresh state root for a fresh repository.
fn registry(name: &str) -> (Scratch, TempDir, Registry) {
    let scratch = Scratch::new(name);
    let root = TempDir::new("state");
    let registry = Registry::open(root.path(), scratch.root()).expect("open");
    (scratch, root, registry)
}

#[test]
fn a_registration_round_trips_through_the_registry() {
    let (_scratch, _root, registry) = registry("session-round-trip");

    // The shapes both platforms actually hand a hook: a named pipe on Windows,
    // whose backslashes must survive, and a socket path on unix.
    let pipe = r"\\.\pipe\LOCAL\cc-msg-e616753ecb04ded84d11504b757e8728";
    let written = registration("e4bf7fd6-1414-48d5-a559-a78958b6cbb3", pipe);
    registry.put(&written).expect("put");

    let listed = registry.list().expect("list");
    assert_eq!(listed, vec![written.clone()]);
    assert_eq!(listed[0].socket, pipe, "the pipe path survives whole");
    assert_eq!(listed[0].token, written.token);
}

#[test]
fn two_worktrees_never_share_one_registry() {
    // The reader runs a pane per project, so a note pinned in one tree must
    // never reach the agent working another.
    let here = Scratch::new("session-here");
    let there = Scratch::new("session-there");
    let root = TempDir::new("state");

    let ours = Registry::open(root.path(), here.root()).expect("open here");
    let theirs = Registry::open(root.path(), there.root()).expect("open there");
    assert_ne!(ours.dir(), theirs.dir());

    ours.put(&registration("aaaa-1111", "here.sock"))
        .expect("put");
    assert_eq!(ours.list().expect("ours").len(), 1);
    assert!(theirs.list().expect("theirs").is_empty());
}

#[test]
fn session_end_removes_only_that_session() {
    let (_scratch, _root, registry) = registry("session-end");
    registry
        .put(&registration("aaaa-1111", "one.sock"))
        .expect("one");
    registry
        .put(&registration("bbbb-2222", "two.sock"))
        .expect("two");
    assert_eq!(registry.list().expect("both").len(), 2);

    registry.remove("aaaa-1111").expect("remove");
    let left = registry.list().expect("left");
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].session, "bbbb-2222");

    // Two panes may both clear a session that has already gone.
    registry
        .remove("aaaa-1111")
        .expect("removing it twice is not an error");
}

#[test]
fn a_torn_registration_is_skipped_and_the_rest_still_list() {
    // A session killed mid-write must not cost the reader the sessions that
    // did register: one unusable file is one skipped file.
    let (_scratch, _root, registry) = registry("session-torn");
    registry
        .put(&registration("bbbb-2222", "good.sock"))
        .expect("good");

    fs::create_dir_all(registry.dir()).expect("dir");
    fs::write(
        registry.dir().join("aaaa-1111.session"),
        "not a registration\n",
    )
    .expect("torn");
    fs::write(
        registry.dir().join("cccc-3333.session"),
        "vigia session 1\n",
    )
    .expect("cut short");

    let listed = registry.list().expect("list");
    assert_eq!(listed.len(), 1, "only the whole one lists: {listed:?}");
    assert_eq!(listed[0].session, "bbbb-2222");
}

#[test]
fn a_file_the_registry_would_never_write_is_never_read() {
    // On Windows a device name opens the device, so the registry reads only
    // names it would itself have written.
    let (_scratch, _root, registry) = registry("session-names");
    fs::create_dir_all(registry.dir()).expect("dir");
    fs::write(registry.dir().join("con.session"), "vigia session 1\n").expect("device");
    fs::write(registry.dir().join("notes.txt"), "not ours\n").expect("stranger");

    assert!(registry.list().expect("list").is_empty());
    assert!(
        registry.put(&registration("../escape", "x.sock")).is_err(),
        "a session id that is not one cannot name a file"
    );
}

#[test]
fn the_registry_sits_outside_the_directory_the_store_watches() {
    // The pane watches its store directory, and `Store::watch` raises on that
    // directory's own path. A registration written inside it would wake every
    // pane on the worktree at every session start with nothing to redraw.
    let scratch = Scratch::new("session-outside");
    let root = TempDir::new("state");
    let store = Store::open(root.path(), scratch.root()).expect("store");
    let registry = Registry::open(root.path(), scratch.root()).expect("registry");

    assert!(
        !registry.dir().starts_with(store.dir()),
        "the registry {:?} is under the store {:?}",
        registry.dir(),
        store.dir()
    );
    assert!(!store.dir().starts_with(registry.dir()));

    registry
        .put(&registration("aaaa-1111", "one.sock"))
        .expect("put");
    assert!(
        files_in(store.dir()).is_empty(),
        "registering wrote into the store the pane watches"
    );
}

#[test]
fn an_empty_registry_lists_nothing_rather_than_failing() {
    // The common case by far: a reader who never installed the hook. Nothing
    // is on disk and Enter must not treat that as an error.
    let (_scratch, _root, registry) = registry("session-empty");
    assert!(!registry.dir().exists(), "opening creates nothing");
    assert!(registry.list().expect("list").is_empty());
}

#[test]
fn the_newest_registration_lists_last() {
    // A note goes to every registered session, and the order is the order they
    // arrived, so a listing reads as a history rather than a filesystem order.
    let (_scratch, _root, registry) = registry("session-order");
    let mut older = registration("bbbb-2222", "older.sock");
    older.written = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
    let mut newer = registration("aaaa-1111", "newer.sock");
    newer.written = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000);

    registry.put(&newer).expect("newer");
    registry.put(&older).expect("older");

    let listed = registry.list().expect("list");
    assert_eq!(
        listed
            .iter()
            .map(|r| r.session.as_str())
            .collect::<Vec<_>>(),
        vec!["bbbb-2222", "aaaa-1111"]
    );
}
