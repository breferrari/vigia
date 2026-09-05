//! Notes a reader pins to lines of the diff for the agent, and the store that
//! holds them between the two processes (`SPEC.md` §11.2 B21).
//!
//! Headless: nothing here draws, and nothing here decides which rows are on
//! screen. The pane hands [`resolve`] the rows it drew; the store is one
//! directory per worktree under the reader's state directory, one file per
//! note, and every write is a temp-and-rename so a reader lists whole files
//! or none.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

/// The first line of every note file. A file whose first line differs was
/// written by a `vigia` this one does not know, and is skipped rather than
/// guessed at.
const VERSION_LINE: &str = "vigia note 1";

/// The extension of a note file. A write in flight carries `.tmp` and is
/// never listed.
const NOTE_EXT: &str = "note";

/// How far from its stored number a line is looked for by its text, in rows on
/// its own side, before the note is judged to have lost it.
pub const NEAR: u32 = 8;

/// Which side of the diff a line is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The index side: a removed line, numbered by the index.
    Old,
    /// The worktree side: an added or context line, numbered by the file.
    New,
}

/// Where a note stands on the ladder the agent climbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Written by the reader and not yet listed by the agent.
    Open,
    /// Listed by the agent at least once.
    Seen,
    /// Resolved by the agent with a line of its own; the pane draws the
    /// departure and the server prunes the file.
    Resolved,
}

/// One note, pinned to a line of the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Unique across processes and restarts; the file's name.
    pub id: String,
    /// Repository-relative path of the file the line is in.
    pub path: String,
    /// Which side the line was on when the note was written.
    pub side: Side,
    /// The line's 1-based number on that side when the note was written.
    pub line: u32,
    /// The line's text when the note was written, which is what re-finds it
    /// after an edit above moves the number.
    pub text: String,
    /// What the reader typed.
    pub body: String,
    /// Where it stands.
    pub status: Status,
    /// The agent's line on a resolve, shown once as the note departs.
    pub reply: Option<String>,
    /// When the reader pressed Enter, to the second.
    pub written: SystemTime,
}

/// Where a note's line is among the rows in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The stored number still carries the stored text.
    At(u32),
    /// The stored text was found at another number within [`NEAR`] rows,
    /// so an edit above or below moved the line.
    Moved(u32),
    /// The stored number is drawn with other text and the stored text is not
    /// nearby, which is what the line looks like after the agent edits it.
    Changed,
    /// Neither the number nor the text is among the rows.
    Gone,
}

/// Where the line `note` was pinned to is among `rows`, each a `(number, text)`
/// on the note's side. Whether the file is in the diff at all is the caller's
/// knowledge, so an adrift note is not a placement.
#[must_use]
pub fn resolve(note: &Note, rows: &[(u32, &str)]) -> Placement {
    let text = note.text.as_str();
    if rows.iter().any(|&(n, t)| n == note.line && t == text) {
        return Placement::At(note.line);
    }
    let mut best: Option<(u32, u32)> = None;
    for &(n, t) in rows {
        if t != text {
            continue;
        }
        let distance = n.abs_diff(note.line);
        if distance > NEAR {
            continue;
        }
        // Nearest wins; on a tie the earlier line does, so a repeated line
        // resolves the same way from any starting order of the rows.
        let better = match best {
            None => true,
            Some((d, at)) => distance < d || (distance == d && n < at),
        };
        if better {
            best = Some((distance, n));
        }
    }
    if let Some((_, n)) = best {
        return Placement::Moved(n);
    }
    if rows.iter().any(|&(n, _)| n == note.line) {
        Placement::Changed
    } else {
        Placement::Gone
    }
}

/// The directory `vigia` keeps state under, or `None` when the environment
/// names no home.
///
/// `XDG_STATE_HOME` when it is set, non-empty and absolute, else
/// `~/.local/state`, which is the XDG base directory rule; `LOCALAPPDATA` on
/// Windows, where there is no XDG and that is the per-user local root. The
/// `windows` flag and the `lookup` closure are parameters so a test can ask
/// about a platform and an environment it is not running in.
#[must_use]
pub fn state_root(windows: bool, lookup: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let set = |name: &str| {
        lookup(name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    if windows {
        return set("LOCALAPPDATA").map(|local| Path::new(&local).join("vigia").join("state"));
    }
    if let Some(xdg) = set("XDG_STATE_HOME").filter(|value| Path::new(value).is_absolute()) {
        return Some(Path::new(&xdg).join("vigia"));
    }
    ["HOME", "USERPROFILE"]
        .into_iter()
        .find_map(set)
        .map(|home| Path::new(&home).join(".local").join("state").join("vigia"))
}

/// The store key of `workdir`: forty hex characters of the SHA-1 of its
/// canonical path, so every process that resolves the same worktree lands on
/// one directory whatever spelling it started from.
///
/// # Errors
///
/// The path cannot be canonicalised, which means it does not exist.
pub fn key(workdir: &Path) -> Result<String> {
    let canonical = fs::canonicalize(workdir).map_err(|source| Error::Canonicalise {
        path: workdir.to_owned(),
        source,
    })?;
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(canonical.to_string_lossy().as_bytes());
    let id = hasher
        .try_finalize()
        .map_err(|why| Error::store(workdir, io::Error::other(why)))?;
    Ok(id.to_hex().to_string())
}

/// The notes of one worktree, on disk.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

/// What a listing found: every note that read whole, and every file that did
/// not, with why.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Listing {
    /// Oldest first, then by id.
    pub notes: Vec<Note>,
    /// Files skipped, each with the reason, so the pane can say so once and
    /// the server can report them beside the rest.
    pub skipped: Vec<(PathBuf, String)>,
}

impl Store {
    /// The store for `workdir` under the state `root`. Creates nothing: the
    /// directory appears on the first [`Store::put`].
    ///
    /// # Errors
    ///
    /// `workdir` cannot be canonicalised.
    pub fn open(root: &Path, workdir: &Path) -> Result<Self> {
        Ok(Self {
            dir: root.join(key(workdir)?),
        })
    }

    /// Where the files are, whether or not it exists yet.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// An id no other process or restart produces: microseconds, the process
    /// and a counter, all in hex.
    #[must_use]
    pub fn new_id() -> String {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        format!(
            "{:x}-{:x}-{:x}",
            now.as_micros(),
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// Write `note` whole, creating the directory on the first call. A reader
    /// listing meanwhile sees the previous file or this one, never a part.
    ///
    /// # Errors
    ///
    /// The directory cannot be created or the file cannot be written or moved
    /// into place; the temporary file is removed on the way out.
    pub fn put(&self, note: &Note) -> Result<()> {
        fs::create_dir_all(&self.dir).map_err(|source| Error::store(&self.dir, source))?;
        // The process id keeps two writers of one note from sharing a temporary.
        let tmp = self
            .dir
            .join(format!("{}.{:x}.tmp", note.id, std::process::id()));
        let done = self.path_of(&note.id);
        fs::write(&tmp, encode(note)).map_err(|source| Error::store(&tmp, source))?;
        fs::rename(&tmp, &done).map_err(|source| {
            let _ = fs::remove_file(&tmp);
            Error::store(&done, source)
        })
    }

    /// Delete the note `id`. A note already gone is not an error: two panes on
    /// one worktree may both withdraw it.
    ///
    /// # Errors
    ///
    /// The file exists and cannot be removed.
    pub fn remove(&self, id: &str) -> Result<()> {
        let path = self.path_of(id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::store(&path, source)),
        }
    }

    /// Every note in the store. A store that does not exist yet lists nothing.
    ///
    /// # Errors
    ///
    /// The directory exists and cannot be read.
    pub fn list(&self) -> Result<Listing> {
        let mut listing = Listing::default();
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(listing),
            Err(source) => return Err(Error::store(&self.dir, source)),
        };
        for entry in entries {
            let path = entry
                .map_err(|source| Error::store(&self.dir, source))?
                .path();
            if path.extension().and_then(|ext| ext.to_str()) != Some(NOTE_EXT) {
                continue;
            }
            match fs::read(&path) {
                Ok(bytes) => match decode(&bytes) {
                    Ok(note) => listing.notes.push(note),
                    Err(why) => listing.skipped.push((path, why)),
                },
                // Withdrawn between the directory read and the file read.
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => listing.skipped.push((path, source.to_string())),
            }
        }
        listing
            .notes
            .sort_by(|a, b| a.written.cmp(&b.written).then_with(|| a.id.cmp(&b.id)));
        Ok(listing)
    }

    fn path_of(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.{NOTE_EXT}"))
    }
}

/// The file: a version line, one `key: value` line per single-line field, then
/// the body and the reply each announced with its byte length, so either may
/// hold any line at all.
fn encode(note: &Note) -> String {
    let secs = note
        .written
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let mut out = String::new();
    // Writing to a String cannot fail, so the results are discarded.
    let _ = writeln!(out, "{VERSION_LINE}");
    let _ = writeln!(out, "id: {}", note.id);
    let _ = writeln!(out, "path: {}", note.path);
    let _ = writeln!(out, "side: {}", side_name(note.side));
    let _ = writeln!(out, "line: {}", note.line);
    let _ = writeln!(out, "status: {}", status_name(note.status));
    let _ = writeln!(out, "written: {secs}");
    let _ = writeln!(out, "text: {}", note.text);
    let _ = writeln!(out, "body {}", note.body.len());
    out.push_str(&note.body);
    out.push('\n');
    if let Some(reply) = &note.reply {
        let _ = writeln!(out, "reply {}", reply.len());
        out.push_str(reply);
        out.push('\n');
    }
    out
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Old => "old",
        Side::New => "new",
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Open => "open",
        Status::Seen => "seen",
        Status::Resolved => "resolved",
    }
}

/// Parse one file, or say in a sentence why it cannot be trusted.
fn decode(bytes: &[u8]) -> std::result::Result<Note, String> {
    let mut cursor = Cursor { bytes, at: 0 };
    let version = cursor.line()?;
    if version != VERSION_LINE {
        return Err(format!(
            "written as {version:?}, which this version does not read"
        ));
    }
    let mut id = None;
    let mut path = None;
    let mut side = None;
    let mut line = None;
    let mut status = None;
    let mut written = None;
    let mut text = None;
    let body_len = loop {
        let header = cursor.line()?;
        if let Some(len) = header.strip_prefix("body ") {
            break len
                .parse::<usize>()
                .map_err(|_| format!("body length {len:?} is not a number"))?;
        }
        let (name, value) = header
            .split_once(": ")
            .ok_or_else(|| format!("header line {header:?} is not a field"))?;
        match name {
            "id" => id = Some(value.to_owned()),
            "path" => path = Some(value.to_owned()),
            "side" => {
                side = Some(match value {
                    "old" => Side::Old,
                    "new" => Side::New,
                    other => return Err(format!("side {other:?} is neither old nor new")),
                });
            }
            "line" => {
                line = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| format!("line {value:?} is not a number"))?,
                );
            }
            "status" => {
                status = Some(match value {
                    "open" => Status::Open,
                    "seen" => Status::Seen,
                    "resolved" => Status::Resolved,
                    other => return Err(format!("status {other:?} is not on the ladder")),
                });
            }
            "written" => {
                let secs = value
                    .parse::<u64>()
                    .map_err(|_| format!("written {value:?} is not a number"))?;
                written = Some(UNIX_EPOCH + Duration::from_secs(secs));
            }
            "text" => text = Some(value.to_owned()),
            other => return Err(format!("field {other:?} is not one this version writes")),
        }
    };
    let body = cursor.block(body_len, "body")?;
    let reply = if cursor.at_end() {
        None
    } else {
        let header = cursor.line()?;
        let len = header
            .strip_prefix("reply ")
            .ok_or_else(|| format!("after the body, {header:?} is not a reply"))?
            .parse::<usize>()
            .map_err(|_| format!("reply length in {header:?} is not a number"))?;
        Some(cursor.block(len, "reply")?)
    };
    if !cursor.at_end() {
        return Err("bytes follow the last field".to_owned());
    }
    let missing = |name: &str| format!("the {name} field is missing");
    Ok(Note {
        id: id
            .filter(|id| !id.is_empty())
            .ok_or_else(|| missing("id"))?,
        path: path
            .filter(|p| !p.is_empty())
            .ok_or_else(|| missing("path"))?,
        side: side.ok_or_else(|| missing("side"))?,
        line: line.ok_or_else(|| missing("line"))?,
        text: text.ok_or_else(|| missing("text"))?,
        body,
        status: status.ok_or_else(|| missing("status"))?,
        reply,
        written: written.ok_or_else(|| missing("written"))?,
    })
}

/// A position in the bytes being decoded.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    /// The next line without its newline. A file that ends mid-line was torn.
    fn line(&mut self) -> std::result::Result<&str, String> {
        let rest = &self.bytes[self.at..];
        let end = rest
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| "the file ends in the middle of a line".to_owned())?;
        let line = std::str::from_utf8(&rest[..end])
            .map_err(|_| "a header line is not UTF-8".to_owned())?;
        self.at += end + 1;
        Ok(line)
    }

    /// Exactly `len` bytes followed by a newline, as UTF-8.
    fn block(&mut self, len: usize, what: &str) -> std::result::Result<String, String> {
        let rest = &self.bytes[self.at..];
        if rest.len() < len + 1 || rest[len] != b'\n' {
            return Err(format!(
                "the {what} is shorter than its {len} announced bytes"
            ));
        }
        let block = std::str::from_utf8(&rest[..len])
            .map_err(|_| format!("the {what} is not UTF-8"))?
            .to_owned();
        self.at += len + 1;
        Ok(block)
    }

    fn at_end(&self) -> bool {
        self.at == self.bytes.len()
    }
}
