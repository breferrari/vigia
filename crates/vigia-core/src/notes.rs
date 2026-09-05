//! Notes a reader pins to lines of the diff for the agent, and the store that
//! holds them between the two processes (`SPEC.md` §11.2 B21).
//!
//! Headless: nothing here draws, and nothing here decides which rows are on
//! screen. The pane hands [`resolve`] the rows it drew; the store is one
//! directory per worktree under a root the shell resolves, one file per note,
//! and every write is a temp-and-rename so a reader lists whole files or none.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notify::{EventKind, RecursiveMode, Watcher as _};

use crate::error::{Error, Result};
use crate::hunk::LineKind;
use crate::watch::roots_of;

/// The first line of every note file. A file whose first line differs was
/// written by a `vigia` this one does not know, and is skipped rather than
/// guessed at.
const VERSION_LINE: &str = "vigia note 1";

/// The extension of a note file. A write in flight carries `.tmp` and is
/// never listed.
const NOTE_EXT: &str = "note";

/// The longest id the store accepts.
const ID_MAX: usize = 64;

/// How far from its stored number a line is looked for by its text, in rows on
/// its own side, before the note is judged to have lost it.
pub const NEAR: u32 = 8;

/// One per process, so two writes in one process never share a temporary and
/// two ids minted in one microsecond differ.
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Which side of the diff a line is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The index side: a removed line, numbered by the index.
    Old,
    /// The worktree side: an added or context line, numbered by the file.
    New,
}

impl Side {
    /// The side a line of `kind` is numbered on.
    #[must_use]
    pub fn of(kind: LineKind) -> Self {
        match kind {
            LineKind::Removed => Self::Old,
            LineKind::Added | LineKind::Context => Self::New,
        }
    }

    /// The word the file and the agent both read.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Old => "old",
            Self::New => "new",
        }
    }
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

impl Status {
    /// The word the file, the pane and the agent all read.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Seen => "seen",
            Self::Resolved => "resolved",
        }
    }
}

/// One note, pinned to a line of the diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Unique across processes and restarts, and the file's name, so
    /// [`Store`] refuses one it could not name a file after.
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
    let mut number_drawn = false;
    let mut nearest: Option<(u32, u32)> = None;
    for &(n, t) in rows {
        number_drawn |= n == note.line;
        if t != note.text {
            continue;
        }
        if n == note.line {
            return Placement::At(n);
        }
        let distance = n.abs_diff(note.line);
        // Nearest wins; on a tie the earlier line does, so a repeated line
        // resolves the same way whatever order the rows arrive in.
        let closer = nearest.is_none_or(|(d, at)| distance < d || (distance == d && n < at));
        if distance <= NEAR && closer {
            nearest = Some((distance, n));
        }
    }
    match nearest {
        Some((_, n)) => Placement::Moved(n),
        None if number_drawn => Placement::Changed,
        None => Placement::Gone,
    }
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
    hasher.update(&os_bytes(canonical.as_os_str()));
    // The only failure is a detected collision attack in the bytes hashed,
    // which are a path this process resolved; it is reported as the store
    // failing at that path because the store is what cannot be opened.
    let id = hasher
        .try_finalize()
        .map_err(|why| Error::store(workdir, io::Error::other(why)))?;
    Ok(id.to_hex().to_string())
}

/// The bytes of a path as the platform holds them, so two paths that differ
/// only where UTF-8 cannot represent them still hash apart.
#[cfg(unix)]
fn os_bytes(path: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_bytes(path: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;
    path.encode_wide().flat_map(u16::to_le_bytes).collect()
}

/// Whether `id` can be a file name inside the store and nothing else:
/// lowercase ASCII letters, digits and dashes, so a filesystem that folds case
/// cannot give one file two names; and not a Windows device name, which is a
/// device whatever extension follows it, refused on every platform so the
/// store stays one rule.
fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ID_MAX
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !is_device(id)
}

fn is_device(id: &str) -> bool {
    let upper = id.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
}

/// The notes of one worktree, on disk.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

/// An armed watch over a store's files, alive for as long as it is held.
pub struct StoreWatch {
    _backend: notify::RecommendedWatcher,
}

/// Whether an event at `path` is about the store at `dir`: the directory
/// itself appearing, or a note file inside it. A temporary in flight is not,
/// so one write is one event rather than two.
fn concerns(dir: &Path, path: &Path) -> bool {
    path == dir || (path.starts_with(dir) && path.extension().is_some_and(|ext| ext == NOTE_EXT))
}

/// What a listing found: every note that read whole, and every file that did
/// not, with why.
#[derive(Debug, Default)]
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
    /// The id is not one the store can name a file after; the directory cannot
    /// be created; the file cannot be written or moved into place, in which
    /// case the temporary is removed on the way out.
    pub fn put(&self, note: &Note) -> Result<()> {
        self.write(note, false).map(|_| ())
    }

    /// Write `note` over the file it already has, and only while that file is
    /// still there, so a note the reader withdrew between the read and this
    /// write is not brought back; `false` says it was gone. The check sits
    /// right before the rename and no closer: two processes with no lock
    /// between them leave the rename itself as the window a withdrawal can
    /// still slip into.
    ///
    /// # Errors
    ///
    /// As [`Store::put`].
    pub fn rewrite(&self, note: &Note) -> Result<bool> {
        self.write(note, true)
    }

    fn write(&self, note: &Note, only_present: bool) -> Result<bool> {
        let done = self.path_of(&note.id)?;
        if only_present && !done.is_file() {
            return Ok(false);
        }
        fs::create_dir_all(&self.dir).map_err(|source| Error::store(&self.dir, source))?;
        let tmp = self.dir.join(format!(
            "{}.{:x}-{:x}.tmp",
            note.id,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&tmp, encode(note)).map_err(|source| Error::store(&tmp, source))?;
        if only_present && !done.is_file() {
            let _ = fs::remove_file(&tmp);
            return Ok(false);
        }
        fs::rename(&tmp, &done).map_err(|source| {
            let _ = fs::remove_file(&tmp);
            Error::store(&done, source)
        })?;
        Ok(true)
    }

    /// Delete the note `id`. A note already gone is not an error: two panes on
    /// one worktree may both withdraw it.
    ///
    /// # Errors
    ///
    /// The id is not one the store names files after, or the file exists and
    /// cannot be removed.
    pub fn remove(&self, id: &str) -> Result<()> {
        let path = self.path_of(id)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::store(&path, source)),
        }
    }

    /// The note `id`, or `None` when the store holds none: the file is not
    /// there, or the id is one the store would never name a file after.
    ///
    /// # Errors
    ///
    /// The file exists and cannot be read, or reads as something other than a
    /// note of this version.
    pub fn get(&self, id: &str) -> Result<Option<Note>> {
        let Ok(path) = self.path_of(id) else {
            return Ok(None);
        };
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(Error::store(&path, source)),
        };
        decode(&bytes)
            .ok()
            .filter(|note| note.id == id)
            .map(Some)
            .ok_or_else(|| {
                let why = io::Error::new(
                    io::ErrorKind::InvalidData,
                    "is not a note this version reads",
                );
                Error::store(&path, why)
            })
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
            let entry = entry.map_err(|source| Error::store(&self.dir, source))?;
            let name = entry.file_name();
            let file = Path::new(&name);
            if file.extension().and_then(|ext| ext.to_str()) != Some(NOTE_EXT) {
                continue;
            }
            let path = entry.path();
            // A name the store would not write is not read either: on Windows a
            // device name opens the device.
            let Some(stem) = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| is_id(stem))
            else {
                listing
                    .skipped
                    .push((path, "is not named by a note id".to_owned()));
                continue;
            };
            match fs::read(&path) {
                Ok(bytes) => match decode(&bytes) {
                    Ok(note) if note.id == stem => listing.notes.push(note),
                    Ok(note) => {
                        let why = format!("names itself {:?}, which is not its file name", note.id);
                        listing.skipped.push((path, why));
                    }
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

    /// Call `changed` from the platform's watch thread whenever a note file
    /// under this store is written, moved in or removed, or the store's own
    /// directory appears. The watch arms on the directory when it exists, on
    /// the state root above it when only that does, and on nothing when neither
    /// is there yet, which is `Ok(None)`: the caller asks again once something
    /// has written. A directory that does not exist cannot be watched, and
    /// creating one for the watch would give a reader who never writes a note a
    /// directory per project.
    ///
    /// # Errors
    ///
    /// The platform's watcher cannot be created or cannot watch the directory.
    pub fn watch(&self, changed: impl Fn() + Send + 'static) -> Result<Option<StoreWatch>> {
        let (target, mode) = if self.dir.is_dir() {
            (self.dir.clone(), RecursiveMode::NonRecursive)
        } else if let Some(root) = self.dir.parent().filter(|root| root.is_dir()) {
            (root.to_path_buf(), RecursiveMode::Recursive)
        } else {
            return Ok(None);
        };
        let mut dirs = roots_of(&self.dir);
        // The directory may not exist yet, so its resolved spelling has to come
        // from the root above it, which does: on macOS the temporary root is a
        // symlink and every event carries the resolved path.
        if let (Some(parent), Some(name)) = (self.dir.parent(), self.dir.file_name())
            && let Ok(resolved) = parent.canonicalize()
        {
            let spelled = resolved.join(name);
            if !dirs.contains(&spelled) {
                dirs.push(spelled);
            }
        }
        let mut backend = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else {
                return;
            };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            if event
                .paths
                .iter()
                .any(|path| dirs.iter().any(|dir| concerns(dir, path)))
            {
                changed();
            }
        })
        .map_err(|e| Error::Watch(Box::new(e)))?;
        backend
            .watch(&target, mode)
            .map_err(|e| Error::Watch(Box::new(e)))?;
        Ok(Some(StoreWatch { _backend: backend }))
    }

    /// The file of `id`, or the error a caller can show when `id` could name
    /// something outside the store.
    fn path_of(&self, id: &str) -> Result<PathBuf> {
        if !is_id(id) {
            let why = io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{id:?} is not a note id"),
            );
            return Err(Error::store(&self.dir, why));
        }
        Ok(self.dir.join(format!("{id}.{NOTE_EXT}")))
    }
}

/// The file: a version line, one `key: value` line per field that cannot hold
/// a newline, then every free-text field announced with its byte length, so
/// no path or line of code can forge a header and a file cut short announces
/// more bytes than follow.
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
    let _ = writeln!(out, "side: {}", note.side.name());
    let _ = writeln!(out, "line: {}", note.line);
    let _ = writeln!(out, "status: {}", note.status.name());
    let _ = writeln!(out, "written: {secs}");
    let mut block = |name: &str, text: &str| {
        let _ = writeln!(out, "{name} {}", text.len());
        out.push_str(text);
        out.push('\n');
    };
    block("path", &note.path);
    block("text", &note.text);
    block("body", &note.body);
    if let Some(reply) = &note.reply {
        block("reply", reply);
    }
    out
}

/// Parse one file, or say in a sentence why it cannot be trusted. Nothing in
/// a file, however large a number it names, may panic: a corrupt note costs
/// that note and never the process.
fn decode(bytes: &[u8]) -> std::result::Result<Note, String> {
    let mut cursor = Cursor { bytes, at: 0 };
    let version = cursor.line()?;
    if version != VERSION_LINE {
        return Err(format!(
            "written as {version:?}, which this version does not read"
        ));
    }
    let mut id = None;
    let mut side = None;
    let mut line = None;
    let mut status = None;
    let mut written = None;
    loop {
        if cursor.starts_with("path ") {
            break;
        }
        let header = cursor.line()?;
        let (name, value) = header
            .split_once(": ")
            .ok_or_else(|| format!("header line {header:?} is not a field"))?;
        let twice = |taken: bool| {
            if taken {
                Err(format!("the {name} field appears twice"))
            } else {
                Ok(())
            }
        };
        match name {
            "id" if is_id(value) => {
                twice(id.is_some())?;
                id = Some(value.to_owned());
            }
            "id" => return Err(format!("id {value:?} is not a note id")),
            "side" => {
                twice(side.is_some())?;
                side = Some(match value {
                    "old" => Side::Old,
                    "new" => Side::New,
                    other => return Err(format!("side {other:?} is neither old nor new")),
                });
            }
            "line" => {
                twice(line.is_some())?;
                line = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| format!("line {value:?} is not a number"))?,
                );
            }
            "status" => {
                twice(status.is_some())?;
                status = Some(match value {
                    "open" => Status::Open,
                    "seen" => Status::Seen,
                    "resolved" => Status::Resolved,
                    other => return Err(format!("status {other:?} is not on the ladder")),
                });
            }
            "written" => {
                twice(written.is_some())?;
                let secs = value
                    .parse::<u64>()
                    .map_err(|_| format!("written {value:?} is not a number"))?;
                let at = UNIX_EPOCH
                    .checked_add(Duration::from_secs(secs))
                    .ok_or_else(|| format!("written {value:?} is past any time"))?;
                written = Some(at);
            }
            other => return Err(format!("field {other:?} is not one this version writes")),
        }
    }
    let path = cursor.block("path")?;
    let text = cursor.block("text")?;
    let body = cursor.block("body")?;
    let reply = if cursor.at_end() {
        None
    } else {
        Some(cursor.block("reply")?)
    };
    if !cursor.at_end() {
        return Err("bytes follow the last field".to_owned());
    }
    let missing = |name: &str| format!("the {name} field is missing");
    Ok(Note {
        id: id.ok_or_else(|| missing("id"))?,
        path,
        side: side.ok_or_else(|| missing("side"))?,
        line: line.ok_or_else(|| missing("line"))?,
        text,
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
    /// The next line without its newline. A file that ends first, at a line's
    /// end or inside one, was cut short.
    fn line(&mut self) -> std::result::Result<&str, String> {
        let rest = &self.bytes[self.at..];
        let end = rest.iter().position(|&b| b == b'\n').ok_or_else(|| {
            if rest.is_empty() {
                "the file ends before its last field".to_owned()
            } else {
                "the file ends in the middle of a line".to_owned()
            }
        })?;
        let line = std::str::from_utf8(&rest[..end])
            .map_err(|_| "a header line is not UTF-8".to_owned())?;
        self.at += end + 1;
        Ok(line)
    }

    /// Whether the bytes at the cursor begin with `prefix`.
    fn starts_with(&self, prefix: &str) -> bool {
        self.bytes[self.at..].starts_with(prefix.as_bytes())
    }

    /// A block announced as `<what> <len>` on its own line: exactly `len`
    /// bytes, then a newline, as UTF-8.
    fn block(&mut self, what: &str) -> std::result::Result<String, String> {
        let header = self.line()?;
        let len = header
            .strip_prefix(what)
            .and_then(|rest| rest.strip_prefix(' '))
            .ok_or_else(|| format!("{header:?} is not the {what} block"))?
            .parse::<usize>()
            .map_err(|_| format!("the {what} length in {header:?} is not a number"))?;
        let rest = &self.bytes[self.at..];
        if rest.get(len) != Some(&b'\n') {
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
