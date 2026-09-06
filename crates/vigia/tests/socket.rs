//! `SPEC.md` §11.2 B21's send rung: Enter posts the note into the running
//! agent session's socket.
//!
//! The wire is two lines and the channel acknowledges neither, so nothing a
//! drawn cell can show is in here. What the socket receives is the whole
//! subject, and the transport is driven through [`post_each`] so every platform
//! runs the same gates: Windows has no way to stand up a named-pipe server from
//! `std`, and a gate that only ever ran on unix is a gate over one third of the
//! matrix.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use serde_json::Value;
use std::cell::RefCell;
use std::io;
use vigia::post::{
    IN_FLIGHT_MAX, Posted, content, context_for, names_a_pipe, permit, post, post_each, word,
};
use vigia_core::{Note, Registration, Registry, Side, Status, Store};

use support::{Scratch, TempDir, note, registration};

const PATH: &str = "src/watch.rs";

/// A registry on a fresh state root for a fresh repository, with `sessions`
/// registered against it.
fn registry(name: &str, sessions: &[&str]) -> (Scratch, TempDir, Registry) {
    let scratch = Scratch::new(name);
    let root = TempDir::new("state");
    let registry = Registry::open(root.path(), scratch.root()).expect("open");
    for (n, session) in sessions.iter().enumerate() {
        registry
            .put(&registration(session, &format!("socket-{n}")))
            .expect("put");
    }
    (scratch, root, registry)
}

/// Every line one run of [`post_each`] handed the transport, by session.
#[derive(Default)]
struct Wire {
    sent: RefCell<Vec<(String, String)>>,
}

impl Wire {
    /// A transport that takes everything, recording what it took.
    fn taking(&self) -> impl FnMut(&Registration, &str) -> io::Result<()> + '_ {
        move |registration, content| {
            let mut sink = Vec::new();
            vigia::post::write_to(&mut sink, registration, content)?;
            self.sent.borrow_mut().push((
                registration.session.clone(),
                String::from_utf8(sink).expect("the wire is UTF-8"),
            ));
            Ok(())
        }
    }

    /// A transport that refuses, the way a socket whose session has ended does.
    fn dead(&self) -> impl FnMut(&Registration, &str) -> io::Result<()> + '_ {
        move |registration, _| {
            self.sent
                .borrow_mut()
                .push((registration.session.clone(), String::new()));
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }

    fn sessions(&self) -> Vec<String> {
        self.sent
            .borrow()
            .iter()
            .map(|(session, _)| session.clone())
            .collect()
    }

    /// The one connection's lines, which is what the auth-then-frame gate reads.
    fn only(&self) -> Vec<String> {
        let sent = self.sent.borrow();
        assert_eq!(sent.len(), 1, "expected one connection, got {}", sent.len());
        sent[0].1.lines().map(str::to_owned).collect()
    }
}

fn anchored(id: &str, body: &str) -> Note {
    Note {
        path: PATH.to_owned(),
        ..note(id, 5, "    margin.checked_mul(2).unwrap_or(margin)", body)
    }
}

#[test]
fn enter_writes_the_auth_line_then_exactly_one_user_frame() {
    // The issue's own gate. Two lines, in this order, and nothing else: the
    // auth line native Windows requires, then the one frame the session takes.
    let (_scratch, _root, registry) = registry("socket-one", &["aaaa-1111"]);
    let wire = Wire::default();

    let posted = post_each(&registry, || "the note".to_owned(), wire.taking());
    assert_eq!(posted, Posted::Sent);

    let lines = wire.only();
    assert_eq!(lines.len(), 2, "two lines and no more: {lines:?}");

    let auth: Value = serde_json::from_str(&lines[0]).expect("the auth line is JSON");
    assert_eq!(auth["type"], "auth");
    assert_eq!(auth["token"], "token-of-aaaa-1111");

    let frame: Value = serde_json::from_str(&lines[1]).expect("the frame is JSON");
    assert_eq!(frame["type"], "user");
    assert_eq!(frame["message"]["content"], "the note");
}

#[test]
fn the_frame_names_the_registered_session() {
    // Claude Code drops a frame whose session_id is not the receiving session's,
    // so naming it is what stops a registration left behind by a session that
    // has ended from delivering into whichever session next answers its socket.
    let (_scratch, _root, registry) = registry("socket-session", &["aaaa-1111"]);
    let wire = Wire::default();
    post_each(&registry, || "the note".to_owned(), wire.taking());

    let frame: Value = serde_json::from_str(&wire.only()[1]).expect("JSON");
    assert_eq!(frame["session_id"], "aaaa-1111");
}

#[test]
fn the_frame_offers_no_reply_address() {
    // vigia binds no inbox, so there is nothing for `from` to name. The agent's
    // way back is the MCP `resolve` and `reply` the pane already draws.
    let (_scratch, _root, registry) = registry("socket-from", &["aaaa-1111"]);
    let wire = Wire::default();
    post_each(&registry, || "the note".to_owned(), wire.taking());

    let frame: Value = serde_json::from_str(&wire.only()[1]).expect("JSON");
    assert!(frame.get("from").is_none(), "frame carries a from: {frame}");
}

#[test]
fn a_note_body_that_would_break_the_wire_is_carried_whole() {
    // One frame is one line, so a newline or a quote in what the reader typed
    // has to survive as data rather than end the frame early.
    let (_scratch, _root, registry) = registry("socket-escape", &["aaaa-1111"]);
    let wire = Wire::default();
    let typed = "line one\nline \"two\"\\ and a tab\there";
    post_each(&registry, || typed.to_owned(), wire.taking());

    let lines = wire.only();
    assert_eq!(lines.len(), 2, "the body did not end the frame: {lines:?}");
    let frame: Value = serde_json::from_str(&lines[1]).expect("JSON");
    assert_eq!(frame["message"]["content"], typed);
}

#[test]
fn the_content_carries_the_note_its_anchor_and_its_context() {
    let note = anchored("n1", "use saturating_mul and drop the unwrap_or.");
    let context = vec![
        (3, "fn settle(margin: Duration) -> Duration {".to_owned()),
        (4, "    // doubled".to_owned()),
        (5, "    margin.checked_mul(2).unwrap_or(margin)".to_owned()),
        (6, "}".to_owned()),
    ];
    let text = content(&note, &context);

    assert!(text.contains("src/watch.rs:5"), "the anchor: {text}");
    assert!(
        text.contains("use saturating_mul"),
        "the reader's words: {text}"
    );
    assert!(
        text.contains("fn settle(margin"),
        "the lines around it: {text}"
    );
    assert!(
        text.contains("> 5 |"),
        "the anchored line is marked among the rest: {text}"
    );
    assert!(text.contains("n1"), "the id it is resolved by: {text}");
}

#[test]
fn the_content_of_a_note_with_no_context_is_still_whole() {
    // A removed line is not in the working tree, so there is nothing to read
    // around it. The anchor and the words still go.
    let mut note = anchored("n2", "why did this go?");
    note.side = Side::Old;
    let text = content(&note, &[]);

    assert!(text.contains("src/watch.rs:5"));
    assert!(text.contains("why did this go?"));
    assert!(text.contains("n2"));
}

#[test]
fn no_registration_attempts_no_connection() {
    // The common case: a reader who never installed the hook. Nothing is
    // opened, and the footer is told to say nothing at all.
    let (_scratch, _root, registry) = registry("socket-none", &[]);
    let wire = Wire::default();

    let posted = post_each(&registry, || "the note".to_owned(), wire.taking());
    assert_eq!(posted, Posted::Unregistered);
    assert!(wire.sessions().is_empty(), "something was opened");
    assert_eq!(word(Posted::Unregistered), None);
}

#[test]
fn a_dead_socket_costs_one_failed_connect_and_the_note_stays_in_the_store() {
    let scratch = Scratch::new("socket-dead");
    let root = TempDir::new("state");
    let registry = Registry::open(root.path(), scratch.root()).expect("registry");
    let store = Store::open(root.path(), scratch.root()).expect("store");
    registry
        .put(&registration("aaaa-1111", "gone.sock"))
        .expect("put");

    let note = anchored("n1", "still worth saying");
    store.put(&note).expect("the store took it");

    let wire = Wire::default();
    let posted = post_each(&registry, || "the note".to_owned(), wire.dead());

    assert_eq!(posted, Posted::Failed);
    assert_eq!(wire.sessions(), vec!["aaaa-1111"], "exactly one attempt");
    assert_eq!(
        store.list().expect("list").notes,
        vec![note],
        "the note is the reader's and survives the socket"
    );
    // The registration is not pruned: SessionEnd clears it, and dropping a live
    // session over one refusal would cost the reader the rung entirely.
    assert_eq!(registry.list().expect("still registered").len(), 1);
}

#[test]
fn every_registration_on_the_worktree_takes_exactly_one_message() {
    // Two agents on one tree both hear a note about a line of the diff they are
    // both working. One message each, never two, and never a rule about which
    // of them wins.
    let (_scratch, _root, registry) = registry("socket-many", &["aaaa-1111", "bbbb-2222"]);
    let wire = Wire::default();

    let posted = post_each(&registry, || "the note".to_owned(), wire.taking());
    assert_eq!(posted, Posted::Sent);

    let mut sessions = wire.sessions();
    sessions.sort();
    assert_eq!(sessions, vec!["aaaa-1111", "bbbb-2222"]);
}

#[test]
fn one_session_taking_it_is_sent_even_when_another_refuses() {
    let (_scratch, _root, registry) = registry("socket-mixed", &["aaaa-1111", "bbbb-2222"]);
    let taken = RefCell::new(Vec::new());

    let posted = post_each(
        &registry,
        || "the note".to_owned(),
        |registration, _| {
            if registration.session == "aaaa-1111" {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            taken.borrow_mut().push(registration.session.clone());
            Ok(())
        },
    );

    assert_eq!(posted, Posted::Sent, "one took it, so it was sent");
    assert_eq!(taken.into_inner(), vec!["bbbb-2222"]);
}

#[test]
fn the_footer_says_sent_when_one_took_it_and_noted_when_none_did() {
    // `sent` cannot mean delivered: the channel writes nothing back, so the
    // strongest true claim is that the line left this process.
    assert_eq!(word(Posted::Sent), Some("sent"));
    assert_eq!(word(Posted::Failed), Some("noted"));
    assert_eq!(word(Posted::Unregistered), None);
}

#[test]
fn a_socket_that_is_not_there_is_refused_rather_than_waited_on() {
    // The real transport, on a path nothing is listening at. What matters is
    // that it comes back at all: the pane must never learn about a dead session
    // by waiting for one.
    let root = TempDir::new("dead-socket");
    let missing = root.path().join("nothing-here.sock");
    let registration = registration("aaaa-1111", &missing.to_string_lossy());

    let refused = post(&registration, "the note");
    assert!(refused.is_err(), "a socket nothing serves was opened");
}

/// The true transport, which only unix can stand a server up for from `std`.
/// Windows named pipes have no server side outside the platform API, so the
/// gates above carry that leg and this one proves the bytes reach a real peer.
#[cfg(unix)]
#[test]
fn a_real_socket_receives_the_two_lines() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;

    let root = TempDir::new("real-socket");
    let path = root.path().join("inbox.sock");
    let listener = UnixListener::bind(&path).expect("bind");

    let taker = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        BufReader::new(stream)
            .lines()
            .map(|line| line.expect("line"))
            .collect::<Vec<_>>()
    });

    let registration = registration("aaaa-1111", &path.to_string_lossy());
    post(&registration, "the note").expect("post");

    let lines = taker.join().expect("the taker");
    assert_eq!(lines.len(), 2, "{lines:?}");
    let auth: Value = serde_json::from_str(&lines[0]).expect("auth");
    assert_eq!(auth["token"], "token-of-aaaa-1111");
    let frame: Value = serde_json::from_str(&lines[1]).expect("frame");
    assert_eq!(frame["message"]["content"], "the note");
}

#[test]
fn a_note_is_posted_only_once_its_words_are_in_the_store() {
    // Ruling 6: the store is written first either way, so a note the socket
    // never took is still the reader's. This is the ordering, asserted where
    // the two acts meet.
    let scratch = Scratch::new("socket-order");
    let root = TempDir::new("state");
    let store = Store::open(root.path(), scratch.root()).expect("store");
    let registry = Registry::open(root.path(), scratch.root()).expect("registry");
    registry
        .put(&registration("aaaa-1111", "one.sock"))
        .expect("put");

    let note = anchored("n1", "the words");
    store.put(&note).expect("put");

    let on_disk = RefCell::new(Vec::new());
    post_each(
        &registry,
        || "the note".to_owned(),
        |_, _| {
            on_disk.borrow_mut().extend(
                store
                    .list()
                    .expect("list")
                    .notes
                    .iter()
                    .map(|n| n.id.clone()),
            );
            Ok(())
        },
    );

    assert_eq!(
        on_disk.into_inner(),
        vec!["n1"],
        "the note was not in the store when the socket was opened"
    );
    assert_eq!(
        store.get("n1").expect("get").map(|n| n.status),
        Some(Status::Open)
    );
}

#[test]
fn the_written_time_orders_nothing_the_wire_sees() {
    // A registration's timestamp orders the listing and nothing else: it must
    // never reach the frame, where it would be a fact about the reader's
    // machine going to the agent.
    let (_scratch, _root, registry) = registry("socket-time", &["aaaa-1111"]);
    let wire = Wire::default();
    post_each(&registry, || "the note".to_owned(), wire.taking());

    let frame: Value = serde_json::from_str(&wire.only()[1]).expect("JSON");
    let rendered = frame.to_string();
    assert!(
        !rendered.contains("1700000000"),
        "the frame carries a timestamp: {rendered}"
    );
}

#[test]
fn only_a_named_pipe_is_ever_opened_on_windows() {
    // `OpenOptions` opens an ordinary file as willingly as a pipe, so without
    // this check a stale or hand-edited registration naming a real file would
    // have the frame written over its first bytes and the footer would say it
    // was sent.
    for pipe in [
        r"\\.\pipe\LOCAL\cc-msg-e616753ecb04ded84d11504b757e8728",
        r"\\.\PIPE\cc-msg-abc",
        r"\\server\pipe\something",
        "//./pipe/cc-msg-abc",
    ] {
        assert!(names_a_pipe(pipe), "{pipe:?} is a named pipe");
    }
    for not in [
        r"C:\Users\me\notes.txt",
        r"C:\pipe\notes.txt",
        "inbox.sock",
        "",
        r"\\.\NUL",
        r"\\.\pipeline\x",
        r"\.\pipe\one-backslash",
        "/tmp/cc-socks/1.sock",
    ] {
        assert!(!names_a_pipe(not), "{not:?} is not a named pipe");
    }
}

#[cfg(windows)]
#[test]
fn a_socket_naming_a_real_file_is_refused_and_the_file_is_untouched() {
    let root = TempDir::new("not-a-pipe");
    let path = root.path().join("notes.txt");
    let was = "the reader's own file\n";
    std::fs::write(&path, was).expect("write");

    let registration = registration("aaaa-1111", &path.to_string_lossy());
    assert!(
        post(&registration, "the note").is_err(),
        "an ordinary file was opened as though it were a pipe"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read back"),
        was,
        "the file was written into"
    );
}

#[test]
fn only_so_many_posts_may_be_in_flight_at_once() {
    // A peer that accepts and never reads holds its thread until the platform
    // gives up, and Windows gives `std` no way to bound that wait, so the bound
    // is on how many such threads can exist.
    let held: Vec<_> = (0..IN_FLIGHT_MAX)
        .map(|_| permit().expect("a slot"))
        .collect();
    assert!(permit().is_none(), "the bound did not hold");

    drop(held);
    assert!(permit().is_some(), "a slot was not given back");
}

#[test]
fn a_registry_that_cannot_be_read_is_noted_rather_than_silent() {
    // Distinct from an empty one: nothing was sent, and the reader should know
    // the note only got as far as the store.
    let scratch = Scratch::new("socket-unreadable");
    let root = TempDir::new("state");
    let registry = Registry::open(root.path(), scratch.root()).expect("registry");

    // A file where the directory should be, which is what a listing cannot read.
    std::fs::create_dir_all(registry.dir().parent().expect("a parent")).expect("parent");
    std::fs::write(registry.dir(), "not a directory").expect("in the way");

    let wire = Wire::default();
    let posted = post_each(&registry, || "the note".to_owned(), wire.taking());
    assert_eq!(posted, Posted::Failed);
    assert_eq!(word(posted), Some("noted"));
    assert!(wire.sessions().is_empty(), "something was opened");
}

#[test]
fn a_note_on_the_old_side_carries_its_anchor_alone() {
    // The line is not in the working tree by definition, so there is nothing to
    // read around it. This is the decision the pane makes on every Enter, and it
    // lives here rather than in the shell so that it has a gate at all.
    let scratch = Scratch::new("socket-context");
    scratch.write(PATH, "one\ntwo\nthree\nfour\nfive\nsix\nseven\n");

    let mut new = anchored("n1", "look here");
    new.line = 4;
    new.text = "four".to_owned();
    let around = context_for(scratch.root(), &new);
    assert!(!around.is_empty(), "the working-tree side has neighbours");
    assert!(
        around
            .iter()
            .any(|(number, text)| *number == 4 && text == "four"),
        "the anchored line is among them: {around:?}"
    );

    let mut old = new.clone();
    old.side = Side::Old;
    assert!(
        context_for(scratch.root(), &old).is_empty(),
        "a removed line has no working-tree neighbours to show"
    );
}
