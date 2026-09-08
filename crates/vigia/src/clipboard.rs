//! The clipboard write, as the escape the terminal answers.
//!
//! `SPEC.md` §11.1. `crossterm` ships this as `clipboard::CopyToClipboard`
//! behind an `osc52` feature that is `dep:base64`, and `crossterm` is not a
//! declared dependency of this workspace at all: it arrives as
//! `ratatui::crossterm`. Taking the feature would mean declaring one crate in
//! order to add another, to buy the formatting of thirty bytes.

use std::ffi::OsStr;
use std::io;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The alphabet, which is the standard one rather than the URL-safe one because
/// OSC 52 carries the payload between delimiters that cannot appear in it.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `text`, base64 encoded.
fn encode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        // Padded to three bytes so the shifts below are one expression rather
        // than three arms; how many of the four characters are real is decided
        // after, by the chunk's own length.
        let [a, b, c] = [
            u32::from(chunk[0]),
            chunk.get(1).copied().map_or(0, u32::from),
            chunk.get(2).copied().map_or(0, u32::from),
        ];
        let packed = a << 16 | b << 8 | c;
        for i in 0..4 {
            // A group of three bytes is four characters; two bytes is three and
            // one byte is two, and the rest is `=`.
            if i <= chunk.len() {
                let at = (packed >> (18 - 6 * i)) & 0b11_1111;
                out.push(ALPHABET[at as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The OSC 52 sequence that puts `text` on the terminal's clipboard.
///
/// **There is no reply**, so nothing downstream can learn whether this worked.
/// A caller telling the reader anything may only say what was sent.
pub fn copy(text: &str) -> String {
    format!("\x1b]52;c;{}\x1b\\", encode(text))
}

/// A way a copy can reach a clipboard.
///
/// Three, and the order they are tried in is the whole design. The escape alone
/// was what shipped, and inside `tmux` it reaches nothing: `set-clipboard` has
/// defaulted to `external` since tmux 2.6, and `external` lets tmux set the
/// terminal's clipboard while forbidding the applications inside it from doing
/// so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The machine's own clipboard, through the tool it ships. Needs nothing of
    /// the terminal and nothing of tmux, and is wrong on a machine the reader is
    /// not sitting at.
    System,
    /// tmux, which passes it on. `tmux load-buffer -w -` makes tmux the one
    /// setting the clipboard, which is what `external` permits.
    Tmux,
    /// The escape, straight to the terminal. The only one that crosses `ssh`.
    Escape,
}

/// The routes this pane tries, in the order it tries them.
///
/// **The system's own tool goes first, and only where the reader is sitting at
/// the machine.** Over `ssh` it would set a clipboard on the far end that nobody
/// can see, and, worse, succeed at it, ending the chain before the escape that
/// would have crossed back. So a remote session skips it and the escape is what
/// carries, which is what it has always been for.
#[must_use]
pub fn plan(tmux: Option<&OsStr>, remote: bool) -> Vec<Route> {
    let mut routes = Vec::with_capacity(3);
    if !remote {
        routes.push(Route::System);
    }
    // Every tmux pane carries `$TMUX` and nothing else sets it. An empty value
    // is not a pane: that is how the variable survives an intermediate shell
    // clearing it.
    if tmux.is_some_and(|value| !value.is_empty()) {
        routes.push(Route::Tmux);
    }
    routes.push(Route::Escape);
    routes
}

/// Whether the reader is at the far end of an `ssh`, where the machine's own
/// clipboard is not the one in front of them.
///
/// Any of the three the daemon sets is enough, since which of them arrives
/// depends on how the session was opened.
#[must_use]
pub fn remote(session: impl Fn(&str) -> bool) -> bool {
    ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|name| session(name))
}

/// The tools this machine's clipboard is set through, best first.
///
/// Empty on a Unix with no display, where there is no clipboard to write to and
/// the escape is the only thing that can carry anything.
#[must_use]
pub fn system_tools(wayland: bool, x11: bool) -> Vec<(&'static str, &'static [&'static str])> {
    if cfg!(target_os = "macos") {
        return vec![("pbcopy", &[])];
    }
    if cfg!(windows) {
        return vec![("clip", &[])];
    }
    let mut tools: Vec<(&str, &'static [&'static str])> = Vec::new();
    if wayland {
        tools.push(("wl-copy", &[]));
    }
    if x11 {
        // Both are ordinary on a desktop and neither is standard, so the one
        // that is absent costs a failed spawn and the next is tried.
        tools.push(("xclip", &["-selection", "clipboard"]));
        tools.push(("xsel", &["--clipboard", "--input"]));
    }
    tools
}

/// The command that hands `tmux` a copy on its standard input.
///
/// `-w` is what makes tmux write the clipboard outward rather than only filling
/// a buffer of its own, and it arrived in tmux 3.2. An older tmux refuses the
/// flag, which is one of the reasons the plan above has something after it.
#[must_use]
pub fn tmux_command() -> Command {
    let mut command = Command::new("tmux");
    command.arg("load-buffer").arg("-w").arg("-");
    command
}

/// What the routes need of the world, so a test can drive every one of them with
/// no terminal, no tmux and no clipboard.
pub trait Carrier {
    /// Hand `text` to this machine's own clipboard tool.
    ///
    /// # Errors
    ///
    /// There is no tool, or the one there is refused it.
    fn to_system(&mut self, text: &str) -> io::Result<()>;

    /// Hand `text` to tmux, and report what tmux made of it.
    ///
    /// # Errors
    ///
    /// tmux is absent, refuses the flag, or does not answer.
    fn to_tmux(&mut self, text: &str) -> io::Result<()>;

    /// Put `sequence` on the wire.
    ///
    /// # Errors
    ///
    /// The write or the flush fails.
    fn to_terminal(&mut self, sequence: &str) -> io::Result<()>;
}

/// Put `text` on the clipboard, trying each of `plan` until one carries it.
///
/// # Errors
///
/// Every route refused, and the error is the last one's, which is the escape's:
/// it is the route every pane has and the only one that can speak for the rest.
pub fn put(carrier: &mut impl Carrier, text: &str, plan: &[Route]) -> io::Result<()> {
    let mut refused = None;
    for route in plan {
        let tried = match route {
            Route::System => carrier.to_system(text),
            Route::Tmux => carrier.to_tmux(text),
            Route::Escape => carrier.to_terminal(&copy(text)),
        };
        match tried {
            Ok(()) => return Ok(()),
            Err(e) => refused = Some(e),
        }
    }
    Err(refused.unwrap_or_else(|| io::Error::other("there was no way to send it")))
}

/// How long the pane waits for a clipboard tool before trying the next route.
///
/// The loop that carries a copy is the loop that paints, so an unbounded wait on
/// a tool that has stopped answering is a frozen pane rather than a slow one. A
/// healthy one answers in single milliseconds; this is three orders above that,
/// because being early is worse than being late here. Giving up early falls
/// through to a route the reader's setup may well discard, which is the defect
/// this whole file is answering.
const PATIENCE: Duration = Duration::from_secs(1);

/// How often the wait above looks, which is how far past it the wait can run.
const LOOK: Duration = Duration::from_millis(5);

impl Carrier for crate::terminal::Session {
    fn to_system(&mut self, text: &str) -> io::Result<()> {
        let tools = system_tools(
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
            std::env::var_os("DISPLAY").is_some(),
        );
        let mut refused = io::Error::other("this machine ships no clipboard tool");
        for (program, args) in tools {
            let mut command = Command::new(program);
            command.args(args);
            match through(command, text) {
                Ok(()) => return Ok(()),
                Err(e) => refused = e,
            }
        }
        Err(refused)
    }

    fn to_tmux(&mut self, text: &str) -> io::Result<()> {
        through(tmux_command(), text)
    }

    fn to_terminal(&mut self, sequence: &str) -> io::Result<()> {
        self.send(sequence)
    }
}

/// Run `command`, hand it `text` on its standard input, and wait out its answer.
fn through(mut command: Command, text: &str) -> io::Result<()> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let handed = hand_over(&mut child, text);
    if handed.is_err() {
        // `Child::drop` neither waits nor kills, so returning without this
        // leaves a child this pane owns for as long as the pane runs.
        let _ = child.kill();
        let _ = child.wait();
    }
    handed
}

/// Write to a spawned child and wait for it, or give up on it.
///
/// The caller buries the child on any error here, which is why this borrows it
/// rather than consuming it.
fn hand_over(child: &mut Child, text: &str) -> io::Result<()> {
    let mut pipe = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("it took no standard input"))?;
    pipe.write_all(text.as_bytes())?;
    // Closed before the wait rather than by falling out of scope after it: these
    // tools read to end of file, so a handle still open here is a wait that would
    // run to the bound below instead of returning at once.
    drop(pipe);
    let until = Instant::now() + PATIENCE;
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                // Short, because a footer is one line and a status carries a
                // signal name and a core-dump note into a sentence that already
                // says how many lines were going.
                Err(io::Error::other("it refused the copy"))
            };
        }
        if Instant::now() >= until {
            return Err(io::Error::other("it did not answer"));
        }
        std::thread::sleep(LOOK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vectors are `crossterm`'s own, read out of its `clipboard.rs` rather
    /// than computed here, so this asserts agreement with the implementation
    /// that was not taken instead of agreement with itself.
    #[test]
    fn the_sequence_is_the_one_crossterm_would_have_written() {
        assert_eq!(copy("foo"), "\x1b]52;c;Zm9v\x1b\\");
    }

    /// The two remainders, which are where a hand-rolled encoder goes wrong and
    /// where a path length lands two times in three.
    #[test]
    fn both_padding_lengths_are_right() {
        for (text, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(text), want, "{text:?} encoded wrong");
        }
    }

    /// A path is not ASCII in general, and the encoder walks bytes rather than
    /// characters, which is the distinction that decides whether it is right.
    #[test]
    fn a_multibyte_path_survives_the_round_trip() {
        for text in ["src/café.rs", "日本語/ファイル.rs", "a/b\u{200b}c.rs"] {
            let encoded = encode(text);
            assert!(
                encoded.is_ascii(),
                "{text:?} encoded to something the escape cannot carry"
            );
            assert_eq!(
                decode(&encoded),
                text.as_bytes(),
                "{text:?} did not survive"
            );
        }
    }

    /// Test-only, and the reason this file has no decoder: nothing in the shell
    /// ever reads a clipboard back, which would be a read of state the reader owns
    /// rather than the write §11.1 licenses.
    fn decode(text: &str) -> Vec<u8> {
        let bits: Vec<u32> = text
            .bytes()
            .filter(|b| *b != b'=')
            .map(|b| ALPHABET.iter().position(|a| *a == b).expect("in alphabet") as u32)
            .collect();
        let mut out = Vec::new();
        for chunk in bits.chunks(4) {
            let mut packed = 0u32;
            for (i, six) in chunk.iter().enumerate() {
                packed |= six << (18 - 6 * i);
            }
            for i in 0..chunk.len() - 1 {
                out.push(((packed >> (16 - 8 * i)) & 0xff) as u8);
            }
        }
        out
    }
}
