//! Enter's second act: the note the store just took, posted into the running
//! agent session's socket (`SPEC.md` §11.2 B21's send rung).
//!
//! The wire is two lines on one connection, and the shape of the second is a
//! fact about Claude Code rather than a choice made here: the receiving side
//! dispatches on `type`, takes `user` frames, and drops one whose
//! `message.content` is missing, empty or not a string. `session_id` is
//! optional and *checked* when present, which is why it is always sent: a
//! registration a session left behind cannot then deliver into whichever
//! session next answers to its socket. Read from the shipped client and
//! confirmed against a live session on 2026-09-06.
//!
//! **Nothing is written back.** The connection is accepted, the frame is taken,
//! and the peer says nothing and holds the connection open, so no word here can
//! ever mean *delivered*. [`word`] says `sent`, which claims only that the line
//! left this process.

use std::io::{self, Write};

use serde_json::json;
use vigia_core::{Note, Registration, Registry};

/// What Enter's post came to, and the only thing the footer is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posted {
    /// No session has registered against this worktree, which is every reader
    /// who has not installed the hook. Nothing was opened.
    Unregistered,
    /// At least one registered session took the line.
    Sent,
    /// Every registered session refused it, or the registry could not be read.
    Failed,
}

/// What the footer says, or `None` for the silence a reader with no hook gets:
/// telling them *noted* on every Enter would be a word about a rung they never
/// asked for.
#[must_use]
pub fn word(posted: Posted) -> Option<&'static str> {
    match posted {
        Posted::Unregistered => None,
        Posted::Sent => Some("sent"),
        Posted::Failed => Some("noted"),
    }
}

/// The first line of the connection. Optional on unix and required on native
/// Windows, where it is also the only way the session verifies the poster.
#[must_use]
pub fn auth_line(token: &str) -> String {
    json!({ "type": "auth", "token": token }).to_string()
}

/// The frame that becomes a turn in the agent's session.
///
/// No `from`: that field is a reply address, `vigia` binds no inbox, and the
/// agent's way back is the `resolve` and `reply` the pane already draws under
/// the note.
#[must_use]
pub fn user_frame(session: &str, content: &str) -> String {
    json!({
        "type": "user",
        "session_id": session,
        "message": { "content": content },
    })
    .to_string()
}

/// What the agent reads: the anchor, the reader's words, the lines around it,
/// and the id to resolve it by. Plain text, because plain text is all the
/// channel carries.
#[must_use]
pub fn content(note: &Note, context: &[(u32, String)]) -> String {
    let mut out = format!(
        "vigia note on {}:{}\n\n{}\n",
        note.path, note.line, note.body
    );
    if !context.is_empty() {
        let width = context
            .iter()
            .map(|(number, _)| number.to_string().len())
            .max()
            .unwrap_or(1);
        out.push('\n');
        for (number, text) in context {
            let mark = if *number == note.line { '>' } else { ' ' };
            out.push_str(&format!("{mark} {number:>width$} | {text}\n"));
        }
    }
    out.push_str(&format!(
        "\nResolve it with the vigia MCP server: resolve(id: {:?}, note: \"<one line saying what \
         you did>\").\n",
        note.id
    ));
    out
}

/// Write the whole conversation into `sink`: the auth line, then one frame,
/// each its own line. This is the entire protocol, so it is what the gates
/// drive rather than a socket that only one platform can stand a server up for.
///
/// # Errors
///
/// `sink` refused a write.
pub fn write_to(
    sink: &mut impl Write,
    registration: &Registration,
    content: &str,
) -> io::Result<()> {
    writeln!(sink, "{}", auth_line(&registration.token))?;
    writeln!(sink, "{}", user_frame(&registration.session, content))?;
    sink.flush()
}

/// Open `registration`'s socket, say the two lines and close.
///
/// The connection is opened only once the message is ready and dropped as soon
/// as it is written, which is what the peer asks for: it closes a connection
/// that has not sent a complete line within thirty seconds.
///
/// # Errors
///
/// The socket cannot be opened, which is what a session that has ended looks
/// like, or the write fails.
pub fn post(registration: &Registration, content: &str) -> io::Result<()> {
    let mut socket = connect(&registration.socket)?;
    write_to(&mut socket, registration, content)
}

/// A unix domain socket, which is what every platform but Windows binds.
#[cfg(unix)]
fn connect(socket: &str) -> io::Result<impl Write> {
    std::os::unix::net::UnixStream::connect(socket)
}

/// A named pipe, which `CreateFile` opens, which is what `OpenOptions` calls.
/// That is the whole of the Windows transport, and why this needs no crate.
#[cfg(windows)]
fn connect(socket: &str) -> io::Result<impl Write> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(socket)
}

/// Post to every session registered against this worktree, through `send`, and
/// say what came of it.
///
/// `content` is a closure and is called only once a registration is in hand,
/// because building the message reads the file the note's neighbours come from
/// and most readers never install the hook: the common Enter must not spend a
/// whole-file read on a message nobody is listening for.
///
/// Every registration gets exactly one message. A registration exists only
/// because a live session ran the hook in this tree, and a note about a line of
/// this diff is for whoever is working it, so there is no rule here about which
/// session wins.
///
/// A refusal is not a reason to forget a registration: `SessionEnd` clears it,
/// and dropping a live session over one busy socket would cost the reader the
/// rung entirely.
pub fn post_each(
    registry: &Registry,
    content: impl FnOnce() -> String,
    mut send: impl FnMut(&Registration, &str) -> io::Result<()>,
) -> Posted {
    // A registry that cannot be read is not the same as an empty one: nothing
    // was sent and the reader should be told the note only got as far as the
    // store, which is what `Failed` says.
    let Ok(registered) = registry.list() else {
        return Posted::Failed;
    };
    if registered.is_empty() {
        return Posted::Unregistered;
    }
    let content = content();
    let mut sent = false;
    for registration in &registered {
        sent |= send(registration, &content).is_ok();
    }
    if sent { Posted::Sent } else { Posted::Failed }
}

/// [`post_each`] over the real transport.
pub fn post_all(registry: &Registry, content: impl FnOnce() -> String) -> Posted {
    post_each(registry, content, post)
}
