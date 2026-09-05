//! `SPEC.md` §11.2 B21, the agent's side: `vigia mcp` serves the store over
//! stdio, one JSON-RPC message per line.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};
use vigia::mcp::{PROJECT_VAR, PROTOCOL_VERSIONS, RESOURCE_URI, Server};
use vigia::{VERSION, state_root};
use vigia_core::{Note, Side, Status, Store};

use support::{Scratch, TempDir, budget, files_in, note, numbered_lines};

const PATH: &str = "src/watch.rs";
const OTHER: &str = "src/other.rs";

/// The mockup's own line, edited into the fixture as its fifth.
const EDITED: &str = "    margin.checked_mul(2).unwrap_or(margin)";

/// Two committed files; the first has its fifth line changed and the second
/// is untouched, so a note on it is adrift.
fn fixture(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(PATH, numbered_lines(12));
    scratch.write(OTHER, numbered_lines(9));
    scratch.commit_all("baseline");
    scratch.edit_line(PATH, 4, EDITED);
    scratch
}

/// A note pinned anywhere, on either side.
fn pinned(id: &str, path: &str, side: Side, line: u32, text: &str) -> Note {
    Note {
        id: id.to_owned(),
        path: path.to_owned(),
        side,
        line,
        text: text.to_owned(),
        body: format!("about {id}"),
        status: Status::Open,
        reply: None,
        written: UNIX_EPOCH + Duration::from_secs(1_800_000_000),
    }
}

/// The environment the server reads: every home variable at `root`, nothing
/// else, so the store lands under the fixture on every platform.
fn env_at(root: &Path) -> impl Fn(&str) -> Option<String> + '_ {
    move |name| match name {
        "XDG_STATE_HOME" | "LOCALAPPDATA" | "HOME" | "USERPROFILE" => {
            Some(root.to_string_lossy().into_owned())
        }
        _ => None,
    }
}

fn state_of(root: &Path) -> PathBuf {
    state_root(cfg!(windows), env_at(root)).expect("a state root under the fixture")
}

/// A worktree, a state root, and the pane's own handle on the store there.
struct Rig {
    scratch: Scratch,
    root: TempDir,
    store: Store,
}

impl Rig {
    fn new(name: &str) -> Self {
        let scratch = fixture(name);
        let root = TempDir::new("mcp-state");
        let store = Store::open(&state_of(root.path()), scratch.root()).expect("open the store");
        Self {
            scratch,
            root,
            store,
        }
    }

    /// The server as the agent's client would start it, at the project root.
    fn server(&self) -> Server {
        Server::open(Some(self.scratch.root()), env_at(self.root.path()))
    }
}

fn request(server: &mut Server, id: u64, method: &str, params: Value) -> Value {
    let line =
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
    let reply = server
        .handle(&line)
        .unwrap_or_else(|| panic!("{method} got no answer"));
    let value: Value = serde_json::from_str(&reply).expect("the answer is JSON");
    assert_eq!(value["jsonrpc"], "2.0");
    assert_eq!(value["id"], id, "the answer carries the request's id");
    value
}

fn result(server: &mut Server, method: &str, params: Value) -> Value {
    let reply = request(server, 1, method, params);
    assert!(
        reply.get("error").is_none(),
        "{method} was refused: {reply}"
    );
    reply["result"].clone()
}

fn call(server: &mut Server, tool: &str, args: Value) -> Value {
    result(
        server,
        "tools/call",
        json!({ "name": tool, "arguments": args }),
    )
}

fn text_of(answer: &Value) -> String {
    assert_eq!(answer["content"][0]["type"], "text", "{answer}");
    answer["content"][0]["text"]
        .as_str()
        .expect("text content")
        .to_owned()
}

/// The `notes` document, asserting the text content is the same document.
fn document(server: &mut Server, all: bool) -> Value {
    let answer = call(server, "notes", json!({ "all": all }));
    assert_eq!(answer["isError"], false, "{answer}");
    let text: Value = serde_json::from_str(&text_of(&answer)).expect("the text is the document");
    assert_eq!(text, answer["structuredContent"]);
    text
}

fn note_named<'a>(document: &'a Value, id: &str) -> &'a Value {
    document["notes"]
        .as_array()
        .expect("a notes array")
        .iter()
        .find(|note| note["id"] == id)
        .unwrap_or_else(|| panic!("no note {id} in {document}"))
}

fn context_of(note: &Value) -> Vec<(u64, String)> {
    note["context"]
        .as_array()
        .expect("a context array")
        .iter()
        .map(|line| {
            (
                line["line"].as_u64().expect("a line number"),
                line["text"].as_str().expect("a line").to_owned(),
            )
        })
        .collect()
}

fn initialize(server: &mut Server, version: &str) -> Value {
    result(
        server,
        "initialize",
        json!({
            "protocolVersion": version,
            "capabilities": {},
            "clientInfo": { "name": "scripted", "version": "0" },
        }),
    )
}

#[test]
fn the_client_version_is_echoed_when_the_server_speaks_it_and_the_latest_answers_otherwise() {
    let rig = Rig::new("mcp-version");
    let mut server = rig.server();
    assert!(!server.initialised());
    for version in PROTOCOL_VERSIONS {
        assert_eq!(initialize(&mut server, version)["protocolVersion"], version);
    }
    let latest = PROTOCOL_VERSIONS[PROTOCOL_VERSIONS.len() - 1];
    assert_eq!(
        initialize(&mut server, "2026-07-28")["protocolVersion"],
        latest,
        "a revision the server does not speak is answered with the latest it does"
    );
    let answer = result(&mut server, "initialize", json!({}));
    assert_eq!(answer["protocolVersion"], latest);
    assert_eq!(answer["capabilities"]["resources"]["listChanged"], true);
    assert!(answer["capabilities"]["tools"].is_object());
    assert_eq!(answer["serverInfo"]["name"], "vigia");
    assert_eq!(answer["serverInfo"]["version"], VERSION);
    let instructions = answer["instructions"].as_str().expect("instructions");
    assert!(
        instructions.contains("notes") && instructions.contains("resolve"),
        "the instructions teach the loop: {instructions}"
    );
    assert!(server.initialised());
}

#[test]
fn server_discover_is_method_not_found_so_a_dual_era_client_falls_back() {
    let rig = Rig::new("mcp-discover");
    let mut server = rig.server();
    let probe = request(
        &mut server,
        7,
        "server/discover",
        json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } }),
    );
    assert_eq!(probe["error"]["code"], -32601, "{probe}");
    assert!(probe.get("result").is_none());
    let unknown = request(&mut server, 8, "prompts/list", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);

    // Notifications are taken silently, and the server goes on.
    assert_eq!(
        server.handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
        None
    );
    assert_eq!(
        server.handle(
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}"#
        ),
        None
    );
    assert_eq!(result(&mut server, "ping", json!({})), json!({}));
}

#[test]
fn a_parse_error_and_an_invalid_request_are_answered_and_the_server_goes_on() {
    let rig = Rig::new("mcp-parse");
    let mut server = rig.server();
    let parse: Value = serde_json::from_str(&server.handle("{not json").expect("an answer"))
        .expect("the answer is JSON");
    assert_eq!(parse["error"]["code"], -32700, "{parse}");
    assert!(
        parse["id"].is_null(),
        "a line with no id is answered with a null id"
    );

    let invalid: Value = serde_json::from_str(
        &server
            .handle(r#"{"jsonrpc":"2.0","id":3}"#)
            .expect("an answer"),
    )
    .expect("JSON");
    assert_eq!(invalid["error"]["code"], -32600, "{invalid}");
    assert_eq!(invalid["id"], 3);

    assert_eq!(server.handle(""), None);
    assert_eq!(server.handle("   "), None);
    // A response answers a request, and this server sends none.
    assert_eq!(
        server.handle(r#"{"jsonrpc":"2.0","id":9,"result":{}}"#),
        None
    );
    assert_eq!(result(&mut server, "ping", json!({})), json!({}));
}

#[test]
fn notes_on_an_empty_store_answers_an_empty_list_and_no_error() {
    let rig = Rig::new("mcp-empty");
    let mut server = rig.server();
    let document = document(&mut server, false);
    assert_eq!(document["notes"], json!([]));
    assert_eq!(document["skipped"], json!([]));
    assert_eq!(document["warnings"], json!([]));
    assert_eq!(document["pruned"], 0);
    assert_eq!(
        document["worktree"].as_str().map(Path::new),
        Some(rig.scratch.worktree().workdir())
    );
    assert!(
        !rig.store.dir().exists(),
        "a listing creates nothing: the store appears on the first write"
    );
}

#[test]
fn notes_carries_the_placement_resolves_line_changed_and_adrift() {
    let rig = Rig::new("mcp-placement");
    // Every note below was written against the edited file, then a line was
    // inserted at the top, which moves every number by one.
    let edited = fs::read_to_string(rig.scratch.path_of(PATH)).expect("read the fixture");
    rig.scratch.write(PATH, format!("inserted\n{edited}"));
    for note in [
        pinned("at-1", PATH, Side::New, 6, EDITED),
        pinned("moved-1", PATH, Side::New, 5, EDITED),
        pinned("changed-1", PATH, Side::New, 4, "what was here"),
        pinned("gone-1", PATH, Side::New, 11, "line 11"),
        pinned("adrift-1", OTHER, Side::New, 3, "line 3"),
        pinned("old-1", PATH, Side::Old, 5, "line 5"),
    ] {
        rig.store.put(&note).expect("the pane writes");
    }
    let mut server = rig.server();
    let document = document(&mut server, false);
    assert_eq!(document["notes"].as_array().map(Vec::len), Some(6));

    let at = note_named(&document, "at-1");
    assert_eq!(at["placement"], "at");
    assert_eq!(at["resolves"], true);
    assert_eq!(at["line_changed"], false);
    assert_eq!(at["adrift"], false);
    assert_eq!(at["current_line"], 6);
    assert_eq!(at["current_text"], EDITED);
    assert_eq!(at["current_path"], PATH);
    assert_eq!(at["side"], "new");
    assert_eq!(at["line"], 6);
    assert_eq!(at["text"], EDITED);
    assert_eq!(at["body"], "about at-1");
    assert_eq!(at["status"], "seen");
    assert_eq!(at["written"], 1_800_000_000_u64);
    assert!(at["reply"].is_null());
    assert_eq!(
        context_of(at),
        [
            (3, "line 2".to_owned()),
            (4, "line 3".to_owned()),
            (5, "line 4".to_owned()),
            (6, EDITED.to_owned()),
            (7, "line 6".to_owned()),
            (8, "line 7".to_owned()),
            (9, "line 8".to_owned()),
        ],
        "three lines each side, read from the working tree"
    );

    let moved = note_named(&document, "moved-1");
    assert_eq!(moved["placement"], "moved");
    assert_eq!(moved["resolves"], true);
    assert_eq!(moved["line_changed"], false);
    assert_eq!(
        moved["line"], 5,
        "the stored number stays beside the current one"
    );
    assert_eq!(moved["current_line"], 6);
    assert_eq!(moved["current_text"], EDITED);

    let changed = note_named(&document, "changed-1");
    assert_eq!(changed["placement"], "changed");
    assert_eq!(changed["resolves"], true);
    assert_eq!(changed["line_changed"], true);
    assert_eq!(changed["current_line"], 4);
    assert_eq!(
        changed["current_text"], "line 3",
        "the line's new text is the cue to resolve"
    );

    let gone = note_named(&document, "gone-1");
    assert_eq!(gone["placement"], "gone");
    assert_eq!(gone["resolves"], false);
    assert_eq!(gone["line_changed"], false);
    assert_eq!(gone["adrift"], false);
    assert!(gone["current_line"].is_null());
    assert!(gone["current_text"].is_null());
    assert_eq!(
        gone["text"], "line 11",
        "the stored text is what the agent has"
    );
    assert_eq!(
        context_of(gone).first(),
        Some(&(8, "line 7".to_owned())),
        "the file is still there around the stored number"
    );

    let adrift = note_named(&document, "adrift-1");
    assert_eq!(adrift["placement"], "adrift");
    assert_eq!(adrift["adrift"], true);
    assert_eq!(adrift["resolves"], false);
    assert!(adrift["current_path"].is_null());
    assert_eq!(
        context_of(adrift).len(),
        6,
        "lines 1 to 6 of a nine-line file around its third"
    );

    let old = note_named(&document, "old-1");
    assert_eq!(old["side"], "old");
    assert_eq!(old["placement"], "at");
    assert_eq!(old["current_line"], 5);
    assert_eq!(old["current_text"], "line 5");
    assert_eq!(
        context_of(old),
        (2..=8)
            .map(|n| (n, format!("line {n}")))
            .collect::<Vec<_>>(),
        "a removed line's neighbours come from the index side of the diff"
    );
}

#[test]
fn a_renamed_file_carries_its_note_to_the_new_path() {
    let scratch = Scratch::new("mcp-renamed");
    scratch.write("old/name.txt", numbered_lines(30));
    scratch.commit_all("baseline");
    scratch.git(&["mv", "old/name.txt", "new-name.txt"]);
    scratch.git(&["reset", "-q"]);
    scratch.edit_line("new-name.txt", 6, "line seven, edited");
    let root = TempDir::new("mcp-state");
    let store = Store::open(&state_of(root.path()), scratch.root()).expect("open the store");
    store
        .put(&pinned("r-1", "old/name.txt", Side::New, 4, "line 4"))
        .expect("the pane writes");

    let mut server = Server::open(Some(scratch.root()), env_at(root.path()));
    let document = document(&mut server, false);
    let note = note_named(&document, "r-1");
    assert_eq!(note["path"], "old/name.txt", "the stored path stays");
    assert_eq!(note["current_path"], "new-name.txt", "beside the new one");
    assert_eq!(note["placement"], "at");
    assert_eq!(note["adrift"], false);
    assert_eq!(note["current_line"], 4);
    assert_eq!(
        context_of(note).last(),
        Some(&(7, "line seven, edited".to_owned())),
        "the lines around it are read from the file under its new name"
    );
}

#[test]
fn listing_marks_each_open_note_seen_and_reports_the_status_the_pane_now_draws() {
    let rig = Rig::new("mcp-seen");
    rig.store
        .put(&note("open-1", 5, EDITED, "use saturating_mul"))
        .expect("the pane writes");
    let mut server = rig.server();
    let document = document(&mut server, false);
    assert_eq!(note_named(&document, "open-1")["status"], "seen");
    let listing = rig.store.list().expect("list");
    assert_eq!(
        listing.notes[0].status,
        Status::Seen,
        "the file was rewritten"
    );
    assert_eq!(
        listing.notes[0].body, "use saturating_mul",
        "and nothing else moved"
    );
    assert_eq!(
        files_in(rig.store.dir()),
        ["open-1.note"],
        "no temporary was left"
    );
}

#[test]
fn a_resolve_rewrites_the_file_with_the_status_and_the_line() {
    let rig = Rig::new("mcp-resolve");
    rig.store
        .put(&note("open-1", 5, EDITED, "use saturating_mul"))
        .expect("the pane writes");
    let mut server = rig.server();
    let answer = call(
        &mut server,
        "resolve",
        json!({ "id": "open-1", "note": "  swapped for saturating_mul  " }),
    );
    assert_eq!(answer["isError"], false, "{answer}");
    assert!(text_of(&answer).contains("resolved open-1"), "{answer}");
    assert_eq!(answer["structuredContent"]["status"], "resolved");
    let listing = rig.store.list().expect("list");
    assert_eq!(listing.notes[0].status, Status::Resolved);
    assert_eq!(
        listing.notes[0].reply.as_deref(),
        Some("swapped for saturating_mul"),
        "the line is what the departure shows, trimmed"
    );
}

#[test]
fn a_resolve_on_an_id_the_store_no_longer_holds_answers_no_such_note_without_an_error() {
    let rig = Rig::new("mcp-no-such");
    let mut server = rig.server();
    for id in ["gone-already", "../outside", "CON"] {
        let answer = call(
            &mut server,
            "resolve",
            json!({ "id": id, "note": "did the thing" }),
        );
        assert_eq!(answer["isError"], false, "{answer}");
        assert_eq!(text_of(&answer), format!("no such note: {id}"));
        assert_eq!(answer["structuredContent"]["found"], false);
    }
    assert!(
        !rig.store.dir().exists(),
        "nothing was written for a note that is not there"
    );
}

#[test]
fn resolve_without_a_note_is_refused() {
    let rig = Rig::new("mcp-refused");
    rig.store
        .put(&note("open-1", 5, EDITED, "use saturating_mul"))
        .expect("the pane writes");
    let mut server = rig.server();
    for args in [
        json!({ "id": "open-1" }),
        json!({ "id": "open-1", "note": "" }),
        json!({ "id": "open-1", "note": "   " }),
        json!({ "id": "open-1", "note": 7 }),
        json!({ "note": "did it" }),
        json!({}),
    ] {
        let answer = call(&mut server, "resolve", args.clone());
        assert_eq!(
            answer["isError"], true,
            "{args} should be refused: {answer}"
        );
        assert!(!text_of(&answer).is_empty());
    }
    let listing = rig.store.list().expect("list");
    assert_eq!(
        listing.notes[0].status,
        Status::Open,
        "a refusal writes nothing"
    );
    assert!(listing.notes[0].reply.is_none());

    let unknown = request(
        &mut server,
        4,
        "tools/call",
        json!({ "name": "withdraw", "arguments": {} }),
    );
    assert_eq!(
        unknown["error"]["code"], -32602,
        "an unknown tool is a protocol error: {unknown}"
    );
}

#[test]
fn a_resolved_file_is_absent_from_the_default_listing_and_gone_after_the_next_one() {
    let rig = Rig::new("mcp-pruned");
    rig.store
        .put(&note("open-1", 5, EDITED, "one"))
        .expect("put");
    rig.store
        .put(&note("open-2", 6, "line 6", "two"))
        .expect("put");
    let mut server = rig.server();
    call(
        &mut server,
        "resolve",
        json!({ "id": "open-1", "note": "done" }),
    );
    assert_eq!(files_in(rig.store.dir()), ["open-1.note", "open-2.note"]);

    let document = document(&mut server, false);
    let ids: Vec<&str> = document["notes"]
        .as_array()
        .expect("notes")
        .iter()
        .map(|note| note["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids, ["open-2"], "the resolved note is not listed");
    assert_eq!(document["pruned"], 1);
    assert_eq!(
        files_in(rig.store.dir()),
        ["open-2.note"],
        "and its file is gone after the listing"
    );
}

#[test]
fn notes_all_lists_resolved_notes_and_prunes_nothing() {
    let rig = Rig::new("mcp-all");
    rig.store
        .put(&note("open-1", 5, EDITED, "one"))
        .expect("put");
    let mut server = rig.server();
    call(
        &mut server,
        "resolve",
        json!({ "id": "open-1", "note": "done" }),
    );
    let document = document(&mut server, true);
    let resolved = note_named(&document, "open-1");
    assert_eq!(resolved["status"], "resolved");
    assert_eq!(resolved["reply"], "done");
    assert_eq!(document["pruned"], 0);
    assert_eq!(
        files_in(rig.store.dir()),
        ["open-1.note"],
        "reading everything removes nothing"
    );
}

#[test]
fn reply_writes_the_line_without_resolving() {
    let rig = Rig::new("mcp-reply");
    rig.store
        .put(&note("open-1", 5, EDITED, "one"))
        .expect("put");
    let mut server = rig.server();
    let answer = call(
        &mut server,
        "reply",
        json!({ "id": "open-1", "text": "which margin do you mean?" }),
    );
    assert_eq!(answer["isError"], false, "{answer}");
    assert!(text_of(&answer).contains("replied on open-1"));
    let listing = rig.store.list().expect("list");
    assert_eq!(listing.notes[0].status, Status::Seen, "a reply is a read");
    assert_eq!(
        listing.notes[0].reply.as_deref(),
        Some("which margin do you mean?")
    );
    let document = document(&mut server, false);
    let note = note_named(&document, "open-1");
    assert_eq!(note["reply"], "which margin do you mean?");
    assert_eq!(document["pruned"], 0, "an answered note stays");

    let refused = call(&mut server, "reply", json!({ "id": "open-1" }));
    assert_eq!(refused["isError"], true);
    let missing = call(
        &mut server,
        "reply",
        json!({ "id": "never", "text": "hello" }),
    );
    assert_eq!(missing["isError"], false);
    assert_eq!(text_of(&missing), "no such note: never");
}

#[test]
fn a_worktree_that_cannot_be_discovered_makes_every_tool_answer_one_sentence_and_leaves_the_server_up()
 {
    let root = TempDir::new("mcp-state");
    let holder = TempDir::new("mcp-no-repo");
    let mut server = Server::open(Some(holder.path()), env_at(root.path()));
    let why = server
        .refusal()
        .expect("a directory that is not a worktree is a refusal")
        .to_owned();
    assert!(why.contains("not inside a git worktree"), "{why}");
    assert!(server.store().is_none());

    assert_eq!(
        initialize(&mut server, "2025-06-18")["protocolVersion"],
        "2025-06-18"
    );
    assert_eq!(result(&mut server, "ping", json!({})), json!({}));
    assert_eq!(
        result(&mut server, "tools/list", json!({}))["tools"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    for (tool, args) in [
        ("notes", json!({})),
        ("resolve", json!({ "id": "a", "note": "b" })),
        ("reply", json!({ "id": "a", "text": "b" })),
    ] {
        let answer = call(&mut server, tool, args);
        assert_eq!(answer["isError"], true, "{tool}: {answer}");
        assert_eq!(text_of(&answer), why, "{tool} says why in one sentence");
    }
    let read = request(
        &mut server,
        5,
        "resources/read",
        json!({ "uri": RESOURCE_URI }),
    );
    assert_eq!(read["error"]["code"], -32603, "{read}");
    assert_eq!(read["error"]["message"], why);
    assert_eq!(
        result(&mut server, "ping", json!({})),
        json!({}),
        "still up"
    );
}

#[test]
fn a_store_with_no_home_makes_every_tool_answer_one_sentence() {
    let scratch = fixture("mcp-no-home");
    let mut server = Server::open(Some(scratch.root()), |_| None);
    let why = server.refusal().expect("no home is a refusal").to_owned();
    assert!(why.contains("no home to keep a note in"), "{why}");
    let answer = call(&mut server, "notes", json!({}));
    assert_eq!(answer["isError"], true);
    assert_eq!(text_of(&answer), why);
}

#[test]
fn a_torn_file_is_reported_as_unreadable_and_the_rest_are_listed() {
    let rig = Rig::new("mcp-torn");
    rig.store
        .put(&note("whole-1", 5, EDITED, "one"))
        .expect("put");
    fs::write(
        rig.store.dir().join("torn-1.note"),
        "vigia note 1\nid: torn-1\nside: new\n",
    )
    .expect("write a file cut short");
    let mut server = rig.server();
    let document = document(&mut server, false);
    assert_eq!(document["notes"].as_array().map(Vec::len), Some(1));
    assert_eq!(document["notes"][0]["id"], "whole-1");
    let skipped = document["skipped"].as_array().expect("skipped");
    assert_eq!(skipped.len(), 1, "{document}");
    assert!(
        skipped[0]["file"]
            .as_str()
            .is_some_and(|file| file.ends_with("torn-1.note")),
        "{document}"
    );
    assert!(
        skipped[0]["why"]
            .as_str()
            .is_some_and(|why| !why.is_empty()),
        "{document}"
    );
    assert_eq!(
        files_in(rig.store.dir()),
        ["torn-1.note", "whole-1.note"],
        "a file the server cannot read is left for the reader"
    );
}

#[test]
fn the_server_started_from_a_subdirectory_and_the_pane_started_at_the_root_read_one_store() {
    let rig = Rig::new("mcp-subdirectory");
    rig.store
        .put(&note("open-1", 5, EDITED, "one"))
        .expect("the pane writes at the root");
    let mut server = Server::open(Some(&rig.scratch.path_of("src")), env_at(rig.root.path()));
    assert_eq!(server.store().map(Store::dir), Some(rig.store.dir()));
    let document = document(&mut server, false);
    assert_eq!(document["notes"][0]["id"], "open-1");

    // And over the wire, with no project variable at all, from that directory.
    let mut client = Client::spawn(None, rig.root.path(), Some(&rig.scratch.path_of("src")));
    client.request(1, "initialize", init_params("2025-06-18"));
    let listed = client.request(2, "tools/call", json!({ "name": "notes", "arguments": {} }));
    assert_eq!(
        listed["result"]["structuredContent"]["notes"][0]["id"],
        "open-1"
    );
    let (status, stderr, _) = client.finish();
    assert!(status.success(), "{status}: {stderr}");
}

#[test]
fn reading_the_resource_is_the_default_listing_and_another_uri_is_not_found() {
    let rig = Rig::new("mcp-resource");
    rig.store
        .put(&note("open-1", 5, EDITED, "one"))
        .expect("put");
    let mut server = rig.server();
    let listed = result(&mut server, "resources/list", json!({}));
    assert_eq!(listed["resources"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["resources"][0]["uri"], RESOURCE_URI);
    assert_eq!(listed["resources"][0]["mimeType"], "application/json");

    let read = result(
        &mut server,
        "resources/read",
        json!({ "uri": RESOURCE_URI }),
    );
    assert_eq!(read["contents"][0]["uri"], RESOURCE_URI);
    assert_eq!(read["contents"][0]["mimeType"], "application/json");
    let document: Value = serde_json::from_str(read["contents"][0]["text"].as_str().expect("text"))
        .expect("the document");
    assert_eq!(
        note_named(&document, "open-1")["status"],
        "seen",
        "reading is a read"
    );
    assert_eq!(
        rig.store.list().expect("list").notes[0].status,
        Status::Seen
    );

    let missing = request(
        &mut server,
        4,
        "resources/read",
        json!({ "uri": "vigia://elsewhere" }),
    );
    assert_eq!(missing["error"]["code"], -32002, "{missing}");
    assert_eq!(missing["error"]["data"]["uri"], "vigia://elsewhere");
}

/// Make the store refuse a write to `open-1`, and hand back the undo. A
/// read-only directory refuses the temporary on Unix; a read-only note refuses
/// the rename over it on Windows.
#[cfg(unix)]
fn deny_writes(rig: &Rig) -> Box<dyn FnOnce()> {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = rig.store.dir().to_path_buf();
    let was = fs::metadata(&dir).expect("metadata").permissions();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("make it read-only");
    Box::new(move || fs::set_permissions(&dir, was).expect("restore"))
}

#[cfg(windows)]
fn deny_writes(rig: &Rig) -> Box<dyn FnOnce()> {
    let file = rig.store.dir().join("open-1.note");
    let was = fs::metadata(&file).expect("metadata").permissions();
    let mut perms = was.clone();
    perms.set_readonly(true);
    fs::set_permissions(&file, perms).expect("make it read-only");
    Box::new(move || fs::set_permissions(&file, was).expect("restore"))
}

#[cfg(not(any(unix, windows)))]
fn deny_writes(_: &Rig) -> Box<dyn FnOnce()> {
    Box::new(|| {})
}

#[test]
fn an_unwritable_store_is_reported_by_resolve_and_by_the_listing() {
    let rig = Rig::new("mcp-unwritable");
    rig.store
        .put(&note("open-1", 5, EDITED, "one"))
        .expect("put");
    let restore = deny_writes(&rig);
    // Probed by behaviour: a user the platform does not refuse (root, say)
    // has nothing here to assert.
    if rig.store.put(&note("open-1", 5, EDITED, "one")).is_ok() {
        restore();
        println!("skipped: this user still writes here, so nothing can refuse");
        return;
    }

    let mut server = rig.server();
    let answer = call(
        &mut server,
        "resolve",
        json!({ "id": "open-1", "note": "done" }),
    );
    assert_eq!(answer["isError"], true, "{answer}");
    assert!(text_of(&answer).contains("refused the write"), "{answer}");

    let document = document(&mut server, false);
    assert_eq!(
        note_named(&document, "open-1")["status"],
        "open",
        "a seen mark that could not be written is not claimed"
    );
    let warnings = document["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 1, "{document}");
    restore();
}

/// The built binary, driven over its pipes the way the agent's client drives
/// it. Stdout is read on a thread so a wait can be bounded.
struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    stderr: JoinHandle<String>,
    /// Every notification that arrived while a response was awaited.
    notifications: Vec<Value>,
}

fn init_params(version: &str) -> Value {
    json!({
        "protocolVersion": version,
        "capabilities": {},
        "clientInfo": { "name": "scripted", "version": "0" },
    })
}

impl Client {
    fn spawn(project: Option<&Path>, root: &Path, cwd: Option<&Path>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_vigia"));
        command
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("XDG_STATE_HOME", root)
            .env("LOCALAPPDATA", root)
            .env("HOME", root)
            .env("USERPROFILE", root);
        match project {
            Some(project) => command.env(PROJECT_VAR, project),
            None => command.env_remove(PROJECT_VAR),
        };
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = command.spawn().expect("spawn vigia mcp");
        let stdin = child.stdin.take().expect("a piped stdin");
        let stdout = child.stdout.take().expect("a piped stdout");
        let stderr = child.stderr.take().expect("a piped stderr");
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stderr = thread::spawn(move || {
            let mut text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut text);
            text
        });
        Self {
            child,
            stdin,
            lines,
            stderr,
            notifications: Vec::new(),
        }
    }

    fn send(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").expect("write to the server");
        self.stdin.flush().expect("flush");
    }

    /// The next line, which has to be a message, within a bounded wait.
    fn message(&self) -> Value {
        let line = self
            .lines
            .recv_timeout(budget(Duration::from_secs(10)))
            .expect("the server answered in time");
        serde_json::from_str(&line).unwrap_or_else(|e| {
            panic!("stdout carried something that is not a message: {line:?}: {e}")
        })
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string(),
        );
        loop {
            let message = self.message();
            if message.get("id").is_some() {
                assert_eq!(message["id"], id, "{message}");
                return message;
            }
            self.notifications.push(message);
        }
    }

    /// The next notification, skipping nothing: a response here is a defect.
    fn notification(&mut self) -> Value {
        let message = self.message();
        assert!(
            message.get("id").is_none(),
            "expected a notification, got {message}"
        );
        message
    }

    /// Close stdin, which is the shutdown the protocol names, and collect
    /// what the process left.
    fn finish(mut self) -> (ExitStatus, String, Vec<Value>) {
        drop(self.stdin);
        let status = self.child.wait().expect("wait for the server");
        while let Ok(line) = self.lines.recv_timeout(Duration::from_secs(2)) {
            let message: Value = serde_json::from_str(&line).unwrap_or_else(|e| {
                panic!("stdout carried something that is not a message: {line:?}: {e}")
            });
            self.notifications.push(message);
        }
        let stderr = self.stderr.join().expect("the stderr reader");
        (status, stderr, self.notifications)
    }
}

#[test]
fn a_scripted_client_over_pipes_drives_the_handshake_and_every_method() {
    let rig = Rig::new("mcp-pipes");
    rig.store
        .put(&note("open-1", 5, EDITED, "use saturating_mul"))
        .expect("the pane writes a note");
    let mut client = Client::spawn(Some(rig.scratch.root()), rig.root.path(), None);

    let init = client.request(1, "initialize", init_params("2025-06-18"));
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "vigia");
    client.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    assert_eq!(client.request(2, "ping", json!({}))["result"], json!({}));

    let tools = client.request(3, "tools/list", json!({}))["result"]["tools"].clone();
    let names: Vec<&str> = tools
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, ["notes", "resolve", "reply"]);
    for tool in tools.as_array().expect("tools") {
        assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
        let description = tool["description"].as_str().expect("a description");
        assert!(description.len() > 40, "{description}");
    }
    assert_eq!(tools[1]["inputSchema"]["required"], json!(["id", "note"]));
    assert_eq!(tools[2]["inputSchema"]["required"], json!(["id", "text"]));

    let listed = client.request(4, "tools/call", json!({ "name": "notes", "arguments": {} }));
    let answer = &listed["result"];
    assert_eq!(answer["isError"], false, "{answer}");
    let text: Value = serde_json::from_str(&text_of(answer)).expect("the text is the document");
    assert_eq!(text, answer["structuredContent"]);
    assert_eq!(answer["structuredContent"]["notes"][0]["id"], "open-1");
    assert_eq!(answer["structuredContent"]["notes"][0]["status"], "seen");
    assert_eq!(answer["structuredContent"]["notes"][0]["placement"], "at");

    let resources = client.request(5, "resources/list", json!({}))["result"]["resources"].clone();
    assert_eq!(resources[0]["uri"], RESOURCE_URI);
    let read =
        client.request(6, "resources/read", json!({ "uri": RESOURCE_URI }))["result"].clone();
    assert_eq!(read["contents"][0]["uri"], RESOURCE_URI);
    let body: Value = serde_json::from_str(read["contents"][0]["text"].as_str().expect("text"))
        .expect("the document");
    assert_eq!(body["notes"][0]["id"], "open-1");

    let replied = client.request(
        7,
        "tools/call",
        json!({ "name": "reply", "arguments": { "id": "open-1", "text": "which margin?" } }),
    )["result"]
        .clone();
    assert_eq!(replied["isError"], false, "{replied}");
    let resolved = client.request(
        8,
        "tools/call",
        json!({ "name": "resolve", "arguments": { "id": "open-1", "note": "swapped for saturating_mul" } }),
    )["result"]
        .clone();
    assert_eq!(resolved["isError"], false, "{resolved}");
    assert!(text_of(&resolved).contains("resolved open-1"));
    let listing = rig.store.list().expect("list");
    assert_eq!(listing.notes[0].status, Status::Resolved);
    assert_eq!(
        listing.notes[0].reply.as_deref(),
        Some("swapped for saturating_mul")
    );

    let (status, stderr, notifications) = client.finish();
    assert!(
        status.success(),
        "closing stdin ends the server cleanly: {status}"
    );
    assert!(stderr.is_empty(), "nothing was logged: {stderr:?}");
    for notification in &notifications {
        assert_eq!(
            notification["method"], "notifications/resources/list_changed",
            "the only thing the server says unasked is that the store changed: {notification}"
        );
    }
}

#[test]
fn a_change_to_the_store_reaches_the_client_as_list_changed() {
    let rig = Rig::new("mcp-changed");
    let mut client = Client::spawn(Some(rig.scratch.root()), rig.root.path(), None);
    client.request(1, "initialize", init_params("2025-06-18"));
    // The watch arms after the handshake is answered and before the next line
    // is read, so a ping answered means it is armed. The store's directory
    // does not exist yet: the watch is on the state root above it.
    assert_eq!(client.request(2, "ping", json!({}))["result"], json!({}));
    assert!(!rig.store.dir().exists());

    rig.store
        .put(&note("open-1", 5, EDITED, "from the pane"))
        .expect("the pane writes");
    let notification = client.notification();
    assert_eq!(
        notification["method"], "notifications/resources/list_changed",
        "{notification}"
    );

    let (status, stderr, _) = client.finish();
    assert!(status.success(), "{status}: {stderr}");
}

#[test]
fn a_config_file_that_does_not_parse_leaves_the_server_on_the_unstaged_diff() {
    let rig = Rig::new("mcp-config");
    let config = rig.root.path().join(".config").join("vigia");
    fs::create_dir_all(&config).expect("make the config directory");
    fs::write(config.join("config"), "staged\n").expect("write a line with no separator");
    rig.store
        .put(&note("open-1", 5, EDITED, "one"))
        .expect("put");
    let mut server = rig.server();
    let notice = server
        .notice()
        .expect("a config file that does not parse is noticed");
    assert!(notice.contains("unstaged"), "{notice}");
    assert!(server.refusal().is_none(), "the server is up");
    let document = document(&mut server, false);
    assert_eq!(note_named(&document, "open-1")["placement"], "at");
}

#[test]
fn a_note_on_a_line_only_the_staged_diff_holds_is_placed_against_it() {
    // Line 3 changed and staged, line 10 changed in the working tree alone, so
    // the path is in both runs: the unstaged diff holds lines 7 to 12 and the
    // staged one lines 1 to 6.
    let rig = Rig::new("mcp-staged");
    rig.scratch.edit_line(PATH, 2, "staged three");
    rig.scratch.git(&["add", PATH]);
    rig.scratch.edit_line(PATH, 9, "unstaged ten");
    rig.store
        .put(&pinned("staged-1", PATH, Side::New, 3, "staged three"))
        .expect("put");

    let mut server = rig.server();
    let listed = document(&mut server, false);
    let without = note_named(&listed, "staged-1");
    assert_eq!(
        without["placement"], "gone",
        "the staged run is not walked by default"
    );

    let config = rig.root.path().join(".config").join("vigia");
    fs::create_dir_all(&config).expect("make the config directory");
    fs::write(config.join("config"), "staged = on\n").expect("write the view default");
    let mut server = rig.server();
    let again = document(&mut server, false);
    let with = note_named(&again, "staged-1");
    assert_eq!(with["placement"], "at", "{with}");
    assert_eq!(with["current_line"], 3);
    assert_eq!(with["current_text"], "staged three");
}

#[test]
fn a_note_on_a_binary_file_and_an_old_side_note_outside_every_hunk_carry_no_context() {
    let scratch = Scratch::new("mcp-binary");
    let bytes: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
    scratch.write("blob.bin", &bytes);
    scratch.write("src/long.rs", numbered_lines(40));
    scratch.commit_all("baseline");
    let changed: Vec<u8> = (0..=255u8).rev().cycle().take(4096).collect();
    scratch.write("blob.bin", &changed);
    scratch.edit_line("src/long.rs", 29, "thirty, edited");
    let root = TempDir::new("mcp-state");
    let store = Store::open(&state_of(root.path()), scratch.root()).expect("open the store");
    store
        .put(&pinned("bin-1", "blob.bin", Side::New, 1, "x"))
        .expect("put");
    store
        .put(&pinned("old-far", "src/long.rs", Side::Old, 5, "line 5"))
        .expect("put");

    let mut server = Server::open(Some(scratch.root()), env_at(root.path()));
    let document = document(&mut server, false);
    let binary = note_named(&document, "bin-1");
    assert_eq!(binary["placement"], "gone", "a binary file has no rows");
    assert_eq!(binary["adrift"], false, "but it is in the diff");
    assert_eq!(binary["context"], json!([]), "and no text to read around");
    let far = note_named(&document, "old-far");
    assert_eq!(far["placement"], "gone");
    assert_eq!(
        far["context"],
        json!([]),
        "the index side holds nothing within three lines of a line no hunk reaches"
    );
}

#[test]
fn an_invalid_utf8_line_and_a_batch_are_answered_and_the_server_goes_on() {
    let rig = Rig::new("mcp-bytes");
    let mut client = Client::spawn(Some(rig.scratch.root()), rig.root.path(), None);
    client.request(1, "initialize", init_params("2025-06-18"));
    client
        .stdin
        .write_all(&[b'{', 0xFF, 0xFE, b'}', b'\n'])
        .expect("write bytes");
    client.stdin.flush().expect("flush");
    let refused = client.message();
    assert_eq!(refused["error"]["code"], -32700, "{refused}");
    assert!(refused["id"].is_null());
    client.send(r#"[{"jsonrpc":"2.0","id":2,"method":"ping"}]"#);
    let batch = client.message();
    assert_eq!(batch["error"]["code"], -32600, "{batch}");
    assert_eq!(client.request(3, "ping", json!({}))["result"], json!({}));
    let (status, stderr, _) = client.finish();
    assert!(status.success(), "{status}: {stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}
