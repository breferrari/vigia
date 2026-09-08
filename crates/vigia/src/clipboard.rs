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
use std::process::{Command, Stdio};

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

/// Which way a copy reaches the clipboard.
///
/// Two, because inside `tmux` the escape above is discarded. `set-clipboard` has
/// defaulted to `external` since tmux 2.6, and `external` lets tmux set the
/// terminal's clipboard while forbidding the applications inside it from doing
/// so. Handing the text to tmux makes tmux the one setting it, which is the one
/// thing that default permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Straight to the terminal, as OSC 52.
    Escape,
    /// Through tmux, which passes it on.
    Tmux,
}

/// The route a pane takes, given what `$TMUX` holds.
///
/// Every tmux pane carries it, and nothing else sets it. An empty value is not a
/// pane: the variable survives being cleared by an intermediate shell that way.
#[must_use]
pub fn route(tmux: Option<&OsStr>) -> Route {
    match tmux {
        Some(value) if !value.is_empty() => Route::Tmux,
        _ => Route::Escape,
    }
}

/// The command that hands `tmux` a copy on its standard input.
///
/// `-w` is what makes tmux write the clipboard outward rather than only filling
/// a buffer of its own, and it arrived in tmux 3.2. Older tmux refuses the flag,
/// which is one of the two reasons [`put`] falls back.
#[must_use]
pub fn tmux_command() -> Command {
    let mut command = Command::new("tmux");
    command.arg("load-buffer").arg("-w").arg("-");
    command
}

/// What a route needs of the world, so a test can drive both without a terminal
/// and without tmux.
pub trait Carrier {
    /// Hand `text` to tmux, and report what tmux made of it.
    ///
    /// # Errors
    ///
    /// tmux is absent, refuses the flag, or exits non-zero.
    fn to_tmux(&mut self, text: &str) -> io::Result<()>;

    /// Put `sequence` on the wire.
    ///
    /// # Errors
    ///
    /// The write or the flush fails.
    fn to_terminal(&mut self, sequence: &str) -> io::Result<()>;
}

/// Put `text` on the clipboard the way `route` says, and fall back to the escape
/// where that way could not carry it.
///
/// The fallback is not caution. `-w` arrived in tmux 3.2, so an older tmux
/// refuses outright, and the escape is what a reader had before this route
/// existed: falling back leaves them exactly where they were rather than worse.
///
/// # Errors
///
/// Neither route carried it, and the error is the escape's, since that is the
/// one every pane has.
pub fn put(carrier: &mut impl Carrier, text: &str, route: Route) -> io::Result<()> {
    if route == Route::Tmux && carrier.to_tmux(text).is_ok() {
        return Ok(());
    }
    carrier.to_terminal(&copy(text))
}

impl Carrier for crate::terminal::Session {
    fn to_tmux(&mut self, text: &str) -> io::Result<()> {
        let mut child = tmux_command()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let mut pipe = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("tmux took no standard input"))?;
        pipe.write_all(text.as_bytes())?;
        // Closed before the wait, and not by falling out of scope after it:
        // tmux reads to end of file, so a handle still open here is a wait that
        // never returns.
        drop(pipe);
        let status = child.wait()?;
        if status.success() {
            return Ok(());
        }
        Err(io::Error::other(format!("tmux refused it, {status}")))
    }

    fn to_terminal(&mut self, sequence: &str) -> io::Result<()> {
        self.send(sequence)
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
