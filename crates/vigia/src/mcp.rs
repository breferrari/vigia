//! `vigia mcp`: the notes store served to the agent over stdio (`SPEC.md`
//! §11.2 B21).
//!
//! One JSON-RPC message per line in, at most one out, hand-rolled over
//! `serde_json`. The `initialize` handshake revisions are what Claude Code
//! speaks to a stdio server; a client on the per-request revision probes
//! `server/discover` first and falls back to the handshake on the
//! method-not-found it gets here, which is the answer its fallback rule names.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};
use vigia_core::{
    CONTEXT, FileDiff, Frame, Hunk, LineKind, Note, Placement, Side, Status, Store, StoreWatch,
    Worktree, resolve,
};

use crate::config::{self, Config};
use crate::state;
use crate::{VERSION, arm_frame};

/// The handshake revisions this server speaks, oldest first. The shapes it
/// sends are the same in every one of them.
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

/// The one resource, which reads as the default listing.
pub const RESOURCE_URI: &str = "vigia://notes";

/// The variable Claude Code sets in a stdio server's environment to the
/// project root, which is where the pane was started.
pub const PROJECT_VAR: &str = "CLAUDE_PROJECT_DIR";

/// What the client is told the server is for, in the handshake.
const INSTRUCTIONS: &str = "vigia is the live diff monitor in the reader's other pane. A note the \
                            reader pins to a line of the diff is here: call notes, act on the code \
                            it points at, then resolve it by id with one line saying what you did.";

const LIST_CHANGED: &str = r#"{"jsonrpc":"2.0","method":"notifications/resources/list_changed"}"#;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const RESOURCE_NOT_FOUND: i64 = -32002;

/// The server over one worktree's store, or the one sentence that says why
/// there is none. Never fails to open: a client that spawned it deserves an
/// answer to every request, and the sentence is the answer.
pub struct Server {
    site: Result<Site, String>,
    /// The config file did not parse, so the unstaged diff is read alone.
    notice: Option<String>,
    /// Whether `initialize` has been answered, after which the store's changes
    /// may be announced.
    initialised: bool,
}

/// What a server needs to answer: the worktree, its store, and the view
/// defaults that decide what the frame walks.
struct Site {
    worktree: Worktree,
    store: Store,
    config: Config,
}

/// A JSON-RPC failure: the code and its message.
type Refused = (i64, String);

impl Server {
    /// A server for the worktree at `project`, or the working directory when
    /// there is none, reading the environment through `env` the way the pane
    /// does.
    #[must_use]
    pub fn open(project: Option<&Path>, env: impl Fn(&str) -> Option<String>) -> Self {
        let project = project.map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let (config, notice) = match config::from_env(&env) {
            Ok(config) => (config, None),
            Err(e) => (
                Config::default(),
                Some(format!("{e}; reading the unstaged diff alone")),
            ),
        };
        Self {
            site: Site::open(&project, env, config),
            notice,
            initialised: false,
        }
    }

    /// The store, when there is one to serve.
    #[must_use]
    pub fn store(&self) -> Option<&Store> {
        self.site.as_ref().ok().map(|site| &site.store)
    }

    /// Why every tool answers one sentence, when that is what they do.
    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        self.site.as_ref().err().map(String::as_str)
    }

    /// What the config file cost, when it did not parse.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Whether the handshake has happened.
    #[must_use]
    pub fn initialised(&self) -> bool {
        self.initialised
    }

    /// One message in, at most one out: a request gets its response line, a
    /// notification and a response get nothing, and a line that is not JSON
    /// gets a parse error with no id, which is what JSON-RPC says to do.
    pub fn handle(&mut self, line: &str) -> Option<String> {
        if line.trim().is_empty() {
            return None;
        }
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(e) => {
                return Some(error_line(
                    Value::Null,
                    PARSE_ERROR,
                    &format!("parse error: {e}"),
                    None,
                ));
            }
        };
        // A batch is an array, and this revision of the protocol has none.
        if !message.is_object() {
            return Some(error_line(
                Value::Null,
                INVALID_REQUEST,
                "invalid request: not a single message",
                None,
            ));
        }
        // A response answers a request, and this server sends none.
        if message.get("result").is_some() || message.get("error").is_some() {
            return None;
        }
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);
        match (id, method) {
            (None, _) => None,
            (Some(id), None) => Some(error_line(
                id,
                INVALID_REQUEST,
                "invalid request: no method",
                None,
            )),
            (Some(id), Some(method)) => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                Some(match self.request(method, &params) {
                    Ok(result) => result_line(id, result),
                    Err((code, why)) => {
                        let data = (code == RESOURCE_NOT_FOUND)
                            .then(|| json!({ "uri": params.get("uri").cloned() }));
                        error_line(id, code, &why, data)
                    }
                })
            }
        }
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, Refused> {
        match method {
            "initialize" => {
                self.initialised = true;
                Ok(initialize(params))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.call(params),
            "resources/list" => Ok(json!({ "resources": [resource()] })),
            "resources/read" => self.read(params),
            other => Err((METHOD_NOT_FOUND, format!("method not found: {other}"))),
        }
    }

    fn call(&self, params: &Value) -> Result<Value, Refused> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| (INVALID_PARAMS, "tools/call needs a tool name".to_owned()))?;
        let args = params.get("arguments").cloned().unwrap_or(Value::Null);
        // A tool this server does not have is a protocol error whatever the
        // site; a tool it has answers the site's sentence when there is none.
        Ok(match (name, &self.site) {
            ("notes" | "resolve" | "reply", Err(why)) => failed(why),
            ("notes", Ok(site)) => {
                let all = args.get("all").and_then(Value::as_bool).unwrap_or(false);
                match site.listing(all) {
                    Ok(document) => answer(pretty(&document), document),
                    Err(why) => failed(&why),
                }
            }
            ("resolve", Ok(site)) => site.resolve(&args),
            ("reply", Ok(site)) => site.reply(&args),
            (other, _) => {
                return Err((
                    INVALID_PARAMS,
                    format!("unknown tool: {other}; the tools are notes, resolve and reply"),
                ));
            }
        })
    }

    fn read(&self, params: &Value) -> Result<Value, Refused> {
        let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
        if uri != RESOURCE_URI {
            return Err((RESOURCE_NOT_FOUND, format!("resource not found: {uri}")));
        }
        let site = self
            .site
            .as_ref()
            .map_err(|why| (INTERNAL_ERROR, why.clone()))?;
        let document = site.listing(false).map_err(|why| (INTERNAL_ERROR, why))?;
        Ok(json!({
            "contents": [{
                "uri": RESOURCE_URI,
                "mimeType": "application/json",
                "text": pretty(&document),
            }],
        }))
    }
}

impl Site {
    fn open(
        project: &Path,
        env: impl Fn(&str) -> Option<String>,
        config: Config,
    ) -> Result<Self, String> {
        let worktree = Worktree::discover(project)
            .map_err(|e| format!("{} is not inside a git worktree: {e}", project.display()))?;
        let store = state::store_for(worktree.workdir(), env)
            .ok_or_else(state::no_home)?
            .map_err(|e| e.to_string())?;
        // The reader's state root, one directory per user, is made here so the
        // first note ever written on this machine can be announced: a watch
        // needs a directory on disk, and the store's own directory is still the
        // pane's first write. A root that cannot be made fails the first write
        // instead, which is reported there.
        if let Some(root) = store.dir().parent() {
            let _ = fs::create_dir_all(root);
        }
        Ok(Self {
            worktree,
            store,
            config,
        })
    }

    /// The document `notes` and the resource answer: every note placed against
    /// the diff as it is now, open ones marked seen, resolved ones pruned
    /// unless `all` asked to read them.
    fn listing(&self, all: bool) -> Result<Value, String> {
        let mut listing = self.store.list().map_err(|e| e.to_string())?;
        let mut frame = self.worktree.frame();
        arm_frame(&mut frame, self.config);
        frame
            .advance()
            .map_err(|e| format!("could not read the diff: {e}"))?;
        let mut notes = Vec::new();
        let mut warnings = Vec::new();
        let mut pruned = 0;
        for note in &mut listing.notes {
            if note.status == Status::Resolved {
                if all {
                    notes.push(self.describe(&mut frame, note));
                } else {
                    match self.store.remove(&note.id) {
                        Ok(()) => pruned += 1,
                        Err(e) => warnings.push(format!("could not prune {}: {e}", note.id)),
                    }
                }
                continue;
            }
            if note.status == Status::Open {
                note.status = Status::Seen;
                match self.store.rewrite(note) {
                    Ok(true) => {}
                    // Withdrawn by the reader since the listing read it, so it is
                    // not a note any more.
                    Ok(false) => continue,
                    Err(e) => {
                        note.status = Status::Open;
                        warnings.push(format!("could not mark {} seen: {e}", note.id));
                    }
                }
            }
            notes.push(self.describe(&mut frame, note));
        }
        let skipped: Vec<Value> = listing
            .skipped
            .iter()
            .map(|(path, why)| json!({ "file": path.display().to_string(), "why": why }))
            .collect();
        Ok(json!({
            "worktree": self.worktree.workdir().display().to_string(),
            "notes": notes,
            "skipped": skipped,
            "warnings": warnings,
            "pruned": pruned,
        }))
    }

    /// One note as the agent reads it: the anchor as stored, where the line
    /// is now, and the lines around it.
    fn describe(&self, frame: &mut Frame, note: &Note) -> Value {
        let placed = self.place(frame, note);
        let (placement, line_changed) = match placed.placement {
            Some(Placement::At(_)) => ("at", false),
            Some(Placement::Moved(_)) => ("moved", false),
            Some(Placement::Changed) => ("changed", true),
            Some(Placement::Gone) => ("gone", false),
            None => ("adrift", false),
        };
        let written = note
            .written
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let context: Vec<Value> = placed
            .context
            .iter()
            .map(|(line, text)| json!({ "line": line, "text": text }))
            .collect();
        json!({
            "id": note.id,
            "path": note.path,
            "side": note.side.name(),
            "line": note.line,
            "text": note.text,
            "body": note.body,
            "status": note.status.name(),
            "reply": note.reply,
            "written": written,
            "placement": placement,
            "resolves": placed.current_line.is_some(),
            "line_changed": line_changed,
            "adrift": placed.placement.is_none(),
            "current_line": placed.current_line,
            "current_text": placed.current_text,
            "current_path": placed.current_path,
            "context": context,
        })
    }

    /// Where the note's line is in the diff `frame` holds, by the pane's own
    /// rules: the file is looked for under every path it answers to, and a
    /// file under none of them leaves the note adrift. A path in both runs is
    /// two entries, and the anchor carries no run, so the note is placed
    /// against the run its line resolves best in, the unstaged one first.
    fn place(&self, frame: &mut Frame, note: &Note) -> Placed {
        let indices: Vec<usize> = (0..frame.files().len())
            .filter(|&index| frame.files()[index].paths().any(|path| path == note.path))
            .collect();
        if indices.is_empty() {
            let context = match note.side {
                Side::New => self.around(&note.path, note.line),
                Side::Old => Vec::new(),
            };
            return Placed::adrift(context);
        }
        let mut best: Option<Placed> = None;
        for index in indices {
            let Ok((change, diff)) = frame.diff(index) else {
                continue;
            };
            let placed = self.placed_in(change.path.clone(), diff, note);
            if best.as_ref().is_none_or(|held| placed.rank() > held.rank()) {
                best = Some(placed);
            }
        }
        best.unwrap_or_else(|| Placed::adrift(Vec::new()))
    }

    /// The note placed against one file's diff, listed under `current_path`.
    fn placed_in(&self, current_path: String, diff: &FileDiff, note: &Note) -> Placed {
        let rows = diff.rows_on(note.side);
        let placement = resolve(note, &rows);
        let current_line = match placement {
            Placement::At(number) | Placement::Moved(number) => Some(number),
            Placement::Changed => Some(note.line),
            Placement::Gone => None,
        };
        let current_text = current_line.and_then(|number| {
            rows.iter()
                .find(|(at, _)| *at == number)
                .map(|(_, text)| (*text).to_owned())
        });
        // A removed line is nowhere but the index side of the diff, so its
        // neighbours come from there; a line on the working-tree side has the
        // file itself around it.
        let centre = current_line.unwrap_or(note.line);
        let context = match note.side {
            Side::New => self.around(&current_path, centre),
            Side::Old => around_old(diff, centre),
        };
        Placed {
            placement: Some(placement),
            current_line,
            current_text,
            current_path: Some(current_path),
            context,
        }
    }

    /// The working-tree lines within [`CONTEXT`] of `centre`, numbered, and
    /// none when the file cannot be read.
    fn around(&self, path: &str, centre: u32) -> Vec<(u32, String)> {
        let Ok(text) = fs::read_to_string(self.worktree.workdir().join(path)) else {
            return Vec::new();
        };
        let first = centre.saturating_sub(CONTEXT).max(1);
        let last = centre.saturating_add(CONTEXT);
        text.lines()
            .enumerate()
            .map(|(at, line)| (u32::try_from(at + 1).unwrap_or(u32::MAX), line))
            .filter(|(number, _)| (first..=last).contains(number))
            .map(|(number, line)| (number, line.to_owned()))
            .collect()
    }

    fn resolve(&self, args: &Value) -> Value {
        let Some(id) = string_arg(args, "id") else {
            return failed("resolve needs the note's id, which notes lists");
        };
        let Some(line) = string_arg(args, "note") else {
            return failed(
                "resolve needs a note: one line saying what you did, which the reader watches \
                 arrive under theirs",
            );
        };
        self.rewrite(&id, |note| {
            note.status = Status::Resolved;
            note.reply = Some(line);
            format!("resolved {} on {}:{}", note.id, note.path, note.line)
        })
    }

    fn reply(&self, args: &Value) -> Value {
        let Some(id) = string_arg(args, "id") else {
            return failed("reply needs the note's id, which notes lists");
        };
        let Some(text) = string_arg(args, "text") else {
            return failed("reply needs the text to draw under the note");
        };
        self.rewrite(&id, |note| {
            if note.status == Status::Open {
                note.status = Status::Seen;
            }
            note.reply = Some(text);
            format!("replied on {} at {}:{}", note.id, note.path, note.line)
        })
    }

    /// Change the note `id` and write it back, or say that there is no such
    /// note, which is not an error: the reader may have withdrawn it, or a
    /// listing pruned it.
    fn rewrite(&self, id: &str, change: impl FnOnce(&mut Note) -> String) -> Value {
        let mut note = match self.store.get(id) {
            Ok(Some(note)) => note,
            Ok(None) => {
                return answer(
                    format!("no such note: {id}"),
                    json!({ "id": id, "found": false }),
                );
            }
            Err(e) => return failed(&e.to_string()),
        };
        let said = change(&mut note);
        match self.store.rewrite(&note) {
            Ok(true) => answer(
                said,
                json!({ "id": id, "found": true, "status": note.status.name() }),
            ),
            Ok(false) => answer(
                format!("no such note: {id}"),
                json!({ "id": id, "found": false }),
            ),
            Err(e) => failed(&format!("the store refused the write: {e}")),
        }
    }
}

/// The index-side lines the diff holds within [`CONTEXT`] of `centre`.
fn around_old(diff: &FileDiff, centre: u32) -> Vec<(u32, String)> {
    diff.hunks
        .iter()
        .flat_map(Hunk::positions)
        .filter(|(old, _, line)| line.kind != LineKind::Added && old.abs_diff(centre) <= CONTEXT)
        .map(|(old, _, line)| (old, line.text.clone()))
        .collect()
}

/// Where a note's line is now, as the agent is told. `None` for the placement
/// is a file the diff does not hold, which is adrift.
struct Placed {
    placement: Option<Placement>,
    current_line: Option<u32>,
    current_text: Option<String>,
    current_path: Option<String>,
    context: Vec<(u32, String)>,
}

impl Placed {
    fn adrift(context: Vec<(u32, String)>) -> Self {
        Self {
            placement: None,
            current_line: None,
            current_text: None,
            current_path: None,
            context,
        }
    }

    /// How well the line was found, for choosing between two runs of one path.
    fn rank(&self) -> u8 {
        match self.placement {
            Some(Placement::At(_)) => 3,
            Some(Placement::Moved(_)) => 2,
            Some(Placement::Changed) => 1,
            Some(Placement::Gone) | None => 0,
        }
    }
}

fn initialize(params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = asked
        .filter(|version| PROTOCOL_VERSIONS.contains(version))
        .unwrap_or(PROTOCOL_VERSIONS[PROTOCOL_VERSIONS.len() - 1]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {}, "resources": { "listChanged": true } },
        "serverInfo": { "name": "vigia", "version": VERSION },
        "instructions": INSTRUCTIONS,
    })
}

/// The three tools, each described in the sentence that teaches its step of
/// the loop: read, act, resolve with what you did.
fn tools() -> Value {
    json!([
        {
            "name": "notes",
            "description": "List the reader's notes pinned to lines of the diff in the vigia \
                            pane: open ones by default, resolved ones too with all. Each carries \
                            its id, its anchor, the body, where the line is now and the lines \
                            around it. Listing marks each note seen and, unless all is set, \
                            removes the resolved ones; act on the code, then resolve by id with \
                            one line saying what you did.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "all": {
                        "type": "boolean",
                        "description": "List resolved notes too, and prune none.",
                    },
                },
                "additionalProperties": false,
            },
        },
        {
            "name": "resolve",
            "description": "Resolve a note by id with one line saying what you did. The reader \
                            watches that line arrive under their note before it leaves the pane, \
                            so the line is required.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The note's id, from notes." },
                    "note": { "type": "string", "description": "One line saying what you did." },
                },
                "required": ["id", "note"],
                "additionalProperties": false,
            },
        },
        {
            "name": "reply",
            "description": "Answer a note by id without resolving it, for a question back to the \
                            reader. The text draws under the note in the pane.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The note's id, from notes." },
                    "text": { "type": "string", "description": "What to say to the reader." },
                },
                "required": ["id", "text"],
                "additionalProperties": false,
            },
        },
    ])
}

fn resource() -> Value {
    json!({
        "uri": RESOURCE_URI,
        "name": "notes",
        "title": "Notes pinned in the vigia pane",
        "description": "The reader's open notes on lines of the diff, placed against the diff as \
                        it is now. Reading marks them seen and removes the resolved ones.",
        "mimeType": "application/json",
    })
}

/// A tool answer: the text the model reads, and the same thing structured.
fn answer(text: String, structured: Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": structured,
        "isError": false,
    })
}

/// A tool answer that is one sentence saying why there is no other, kept a
/// result rather than a protocol error so the model can correct itself.
fn failed(why: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": why }],
        "isError": true,
    })
}

fn string_arg(args: &Value, name: &str) -> Option<String> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn pretty(document: &Value) -> String {
    serde_json::to_string_pretty(document).unwrap_or_else(|_| document.to_string())
}

fn result_line(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_line(id: Value, code: i64, message: &str, data: Option<Value>) -> String {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error }).to_string()
}

/// Whether the store's changes are being announced, and if not, why not.
enum Armed {
    /// Held for its drop: the watch lives as long as the loop does.
    Watching { _held: StoreWatch },
    /// Nothing on disk to watch yet; asked again after the next request.
    NotYet,
    /// The platform refused, said once; not asked again.
    Failed,
}

/// The loop: read stdin a line at a time until it closes, answer on stdout,
/// and announce the store's changes from the watch thread on the same stdout
/// under one lock, so no two messages interleave.
#[must_use]
pub fn serve() -> ExitCode {
    let project = std::env::var_os(PROJECT_VAR)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let mut server = Server::open(project.as_deref(), |key| std::env::var(key).ok());
    if let Some(notice) = server.notice() {
        eprintln!("vigia mcp: {notice}");
    }
    if let Some(why) = server.refusal() {
        eprintln!("vigia mcp: {why}");
    }
    let out = Arc::new(Mutex::new(io::stdout()));
    let mut armed = Armed::NotYet;
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            // A line that is not UTF-8 is not a message, and the protocol says
            // what to answer; the bytes are consumed, so the next line still reads.
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                let refused =
                    error_line(Value::Null, PARSE_ERROR, &format!("parse error: {e}"), None);
                if emit(&out, &refused).is_err() {
                    return ExitCode::SUCCESS;
                }
                continue;
            }
            Err(e) => {
                eprintln!("vigia mcp: could not read stdin: {e}");
                return ExitCode::FAILURE;
            }
        };
        if let Some(reply) = server.handle(&line)
            && emit(&out, &reply).is_err()
        {
            // The client went away; there is nobody to answer.
            return ExitCode::SUCCESS;
        }
        // Announcing before the handshake would be a message the client did
        // not ask for, so the watch waits for it, and then for the store to
        // exist.
        if matches!(armed, Armed::NotYet) && server.initialised() {
            armed = arm(&server, &out);
        }
    }
    ExitCode::SUCCESS
}

fn arm(server: &Server, out: &Arc<Mutex<io::Stdout>>) -> Armed {
    let Some(store) = server.store() else {
        return Armed::Failed;
    };
    let (tx, rx) = mpsc::channel::<()>();
    let watch = match store.watch(move || {
        let _ = tx.send(());
    }) {
        Ok(Some(watch)) => watch,
        Ok(None) => return Armed::NotYet,
        Err(e) => {
            eprintln!("vigia mcp: cannot watch the store: {e}");
            return Armed::Failed;
        }
    };
    let out = Arc::clone(out);
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            // A burst of events, one per file a listing rewrote, is one
            // announcement: the client re-lists once either way.
            while rx.try_recv().is_ok() {}
            if emit(&out, LIST_CHANGED).is_err() {
                break;
            }
        }
    });
    Armed::Watching { _held: watch }
}

/// One message, one line, flushed, under the lock the watch thread shares.
fn emit(out: &Mutex<io::Stdout>, line: &str) -> io::Result<()> {
    let mut out = out.lock().unwrap_or_else(PoisonError::into_inner);
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}
