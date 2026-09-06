//! Where a running agent session records the socket a note can be posted into
//! (`SPEC.md` §11.2 B21's send rung).
//!
//! The agent's session writes one file here from a hook and clears it when it
//! ends; the pane reads them on Enter. It is keyed by worktree exactly as the
//! notes store is, so a pane and a hook that resolved the same tree land on one
//! directory, and a note pinned in one project can never reach the agent
//! working another.
//!
//! It sits *beside* the store rather than inside it. [`Store::watch`] raises on
//! the store directory's own path, so a registration written in there would
//! wake every pane on the worktree at every session start with nothing on
//! screen to redraw.
//!
//! [`Store::watch`]: crate::Store::watch

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};
use crate::notes::{Cursor, is_id, key, rename_into_place, temp_name};

/// The first line of every registration file. A file whose first line differs
/// was written by a `vigia` this one does not know, and is skipped rather than
/// guessed at.
const VERSION_LINE: &str = "vigia session 1";

/// The extension of a registration file. A write in flight carries `.tmp` and
/// is never listed.
const SESSION_EXT: &str = "session";

/// The directory registrations live in, under the state root and beside the
/// stores.
const SESSIONS_DIR: &str = "sessions";

/// One agent session's inbox, as that session's own hook recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// The session's id, which the frame carries so a registration left behind
    /// by a session that has ended cannot deliver into whichever session next
    /// answers to its socket.
    pub session: String,
    /// The socket to open: a path on unix, a named pipe on Windows.
    pub socket: String,
    /// The per-session key the first line of the connection presents. Secret,
    /// which is why the file is the reader's alone to read.
    pub token: String,
    /// When the hook wrote it.
    pub written: SystemTime,
}

/// The agent sessions registered against one worktree, on disk.
#[derive(Debug, Clone)]
pub struct Registry {
    dir: PathBuf,
}

impl Registry {
    /// The registry for `workdir` under the state `root`. Creates nothing: the
    /// directory appears on the first [`Registry::put`], so a reader who never
    /// runs the hook never gets a directory per project.
    ///
    /// # Errors
    ///
    /// `workdir` cannot be canonicalised.
    pub fn open(root: &Path, workdir: &Path) -> Result<Self> {
        Ok(Self {
            dir: root.join(SESSIONS_DIR).join(key(workdir)?),
        })
    }

    /// Where the files are, whether or not it exists yet.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Record `registration`, replacing whatever that session had before.
    ///
    /// # Errors
    ///
    /// The session is not one the registry can name a file after; the directory
    /// cannot be created; the file cannot be written or moved into place, in
    /// which case the temporary is removed on the way out.
    pub fn put(&self, registration: &Registration) -> Result<()> {
        let done = self.path_of(&registration.session)?;
        fs::create_dir_all(&self.dir).map_err(|source| Error::session(&self.dir, source))?;
        let tmp = self.dir.join(temp_name(&registration.session));
        write_private(&tmp, &encode(registration))
            .map_err(|source| Error::session(&tmp, source))?;
        rename_into_place(&tmp, &done).map_err(|source| Error::session(&done, source))
    }

    /// Forget the session `session`. One already gone is not an error: a
    /// `SessionEnd` hook may run after the file has been cleared some other way.
    ///
    /// # Errors
    ///
    /// The session is not one the registry names files after, or the file exists
    /// and cannot be removed.
    pub fn remove(&self, session: &str) -> Result<()> {
        let path = self.path_of(session)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::session(&path, source)),
        }
    }

    /// Every session registered against this worktree, oldest first. A registry
    /// that does not exist yet lists nothing, which is the common case: most
    /// readers never install the hook.
    ///
    /// A file that cannot be read as a registration is skipped rather than
    /// reported. Unlike a note, nothing here is the reader's words, so there is
    /// nothing to lose and nothing worth a line on the footer: a session killed
    /// mid-write must not cost the reader the sessions that did register. The
    /// cost of that is at the far end: a registry whose every file is unreadable
    /// lists nothing, and the pane cannot tell it from a reader who never
    /// installed the hook.
    ///
    /// # Errors
    ///
    /// The directory exists and cannot be read.
    pub fn list(&self) -> Result<Vec<Registration>> {
        let mut found = Vec::new();
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(found),
            Err(source) => return Err(Error::session(&self.dir, source)),
        };
        for entry in entries {
            let entry = entry.map_err(|source| Error::session(&self.dir, source))?;
            let name = entry.file_name();
            let file = Path::new(&name);
            if file.extension().and_then(|ext| ext.to_str()) != Some(SESSION_EXT) {
                continue;
            }
            // A name the registry would not write is not read either: on Windows
            // a device name opens the device.
            let Some(stem) = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| is_id(stem))
            else {
                continue;
            };
            // The type of the entry itself, which does not follow a link: this
            // reads whole files into memory, and a registration is only ever
            // written here as one.
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            if let Ok(bytes) = fs::read(entry.path())
                && let Some(registration) = decode(&bytes).filter(|it| it.session == stem)
            {
                found.push(registration);
            }
        }
        found.sort_by(|a, b| {
            a.written
                .cmp(&b.written)
                .then_with(|| a.session.cmp(&b.session))
        });
        Ok(found)
    }

    fn path_of(&self, session: &str) -> Result<PathBuf> {
        if !is_id(session) {
            let why = io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{session:?} is not a session id"),
            );
            return Err(Error::session(&self.dir, why));
        }
        Ok(self.dir.join(format!("{session}.{SESSION_EXT}")))
    }
}

/// Write the file, readable by nobody else where the platform has modes.
///
/// The mode is set as the file is created rather than after it is written: the
/// token is the whole of the authority the file carries, and a chmod after the
/// write leaves a window in which anyone on the machine can read it. Windows has
/// no modes and inherits a per-user root instead.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write as _;

    let mut file = create_private(path)?;
    file.write_all(text.as_bytes())
}

/// The name carries this process and a counter, so an existing one is a
/// collision worth failing on rather than a file to write over. Only the mode
/// differs by platform.
#[cfg(unix)]
fn create_private(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;

    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// The file: a version line, the two fields that cannot hold a newline, then
/// the socket and the token announced with their byte lengths, so a path or a
/// token cannot forge a header and a file cut short announces more bytes than
/// follow.
fn encode(registration: &Registration) -> String {
    let secs = registration
        .written
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let mut out = String::new();
    // Writing to a String cannot fail, so the results are discarded.
    let _ = writeln!(out, "{VERSION_LINE}");
    let _ = writeln!(out, "session: {}", registration.session);
    let _ = writeln!(out, "written: {secs}");
    let mut block = |name: &str, text: &str| {
        let _ = writeln!(out, "{name} {}", text.len());
        out.push_str(text);
        out.push('\n');
    };
    block("socket", &registration.socket);
    block("token", &registration.token);
    out
}

/// Parse one file, or `None` when it is not one this version can trust. The
/// fields are in the order [`encode`] writes them, since nothing but that
/// function writes this file. Nothing in a file, however large a number it
/// names, may panic: a corrupt registration costs that session and never the
/// process.
fn decode(bytes: &[u8]) -> Option<Registration> {
    let mut cursor = Cursor::over(bytes);
    if cursor.line().ok()? != VERSION_LINE {
        return None;
    }
    let session = cursor.line().ok()?.strip_prefix("session: ")?.to_owned();
    if !is_id(&session) {
        return None;
    }
    let secs: u64 = cursor
        .line()
        .ok()?
        .strip_prefix("written: ")?
        .parse()
        .ok()?;
    let socket = cursor.block("socket").ok()?;
    let token = cursor.block("token").ok()?;
    if !cursor.at_end() {
        return None;
    }
    Some(Registration {
        session,
        socket,
        token,
        // Checked: adding to a `SystemTime` panics on overflow, and this number
        // came off the disk.
        written: UNIX_EPOCH.checked_add(Duration::from_secs(secs))?,
    })
}
