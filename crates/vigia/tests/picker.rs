//! The picture query against a terminal that never answers. `SPEC.md` §11.1.
#![cfg(unix)]

#[path = "pty/mod.rs"]
mod pty;

use std::io::{Read, Write};
use std::process::Command;
use std::time::{Duration, Instant};

use pty::asked_line;

/// Set in the child this test starts inside a fresh terminal.
const CHILD: &str = "VIGIA_PICKER_CHILD";

/// The child's half: ask, then read one byte from the terminal itself. It can
/// only get the byte if nothing else is still reading.
#[test]
fn silent_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    // Inside raw mode and left after, as the session holds it in `run`.
    ratatui::crossterm::terminal::enable_raw_mode().expect("raw mode");
    let began = Instant::now();
    let (picker, answer) = vigia::picker(true);
    let waited = began.elapsed();
    ratatui::crossterm::terminal::disable_raw_mode().expect("leave raw mode");
    let mut tty = std::fs::File::open("/dev/tty").expect("the child has a terminal");
    let compress = picker
        .capabilities()
        .contains(&ratatui_image::picker::Capability::KittyCompression);
    println!(
        "asked {:.3} {:?} {answer:?} compress: {compress}",
        waited.as_secs_f64() * 1000.0,
        picker.protocol_type()
    );
    std::io::stdout().flush().expect("flush");
    let mut byte = [0u8; 1];
    tty.read_exact(&mut byte).expect("read the byte");
    println!("got {}", byte[0] as char);
}

#[test]
fn silent_terminal_no_reader() {
    let text = asked_in_pty(None);
    assert!(text.contains("got z"), "the typed byte was eaten: {text}");
    let line = asked_line(&text);
    assert!(
        line.contains("Halfblocks"),
        "a silent terminal chose a protocol: {line}"
    );
    let waited = pty::waited_ms(&text);
    assert!(
        waited < vigia::PICTURES_WAIT.as_secs_f64() * 1000.0 + 200.0,
        "the query waited {waited}ms on a silent terminal: {line}"
    );
}

#[test]
fn kitty_answer_read() {
    // What kitty sends back: the graphics reply, the cell size, the status.
    let text = asked_in_pty(Some(b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n"));
    assert!(text.contains("got z"), "the typed byte was eaten: {text}");
    let line = asked_line(&text);
    assert!(
        line.contains("Kitty") && line.contains("kitty: true") && line.contains("cell: true"),
        "an answering kitty was not read: {line}"
    );
}

#[test]
fn kitty_compression_read() {
    // kitty answers the compressed probe as well as the plain one.
    let text = asked_in_pty(Some(
        b"\x1b_Gi=31;OK\x1b\\\x1b_Gi=32;OK\x1b\\\x1b[6;20;10t\x1b[0n",
    ));
    assert!(
        text.contains("a=q,t=d,f=24,o=z;"),
        "the query did not ask whether the terminal takes compression: {text}"
    );
    let line = asked_line(&text);
    assert!(
        line.contains("Kitty") && line.contains("compress: true"),
        "a kitty that takes compression was not read as one: {line}"
    );
    let plain = asked_in_pty(Some(b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n"));
    assert!(
        asked_line(&plain).contains("compress: false"),
        "a kitty that did not answer the probe was read as taking compression"
    );
}

#[test]
fn status_only_halfblocks() {
    let text = asked_in_pty(Some(b"\x1b[0n"));
    assert!(text.contains("got z"), "the typed byte was eaten: {text}");
    let line = asked_line(&text);
    assert!(
        line.contains("Halfblocks") && line.contains("cell: false"),
        "a terminal that answered only its status chose a protocol: {line}"
    );
}

/// Run the child in a fresh terminal that sends `answer` once the query arrives,
/// or nothing, then type a byte; everything the terminal showed.
fn asked_in_pty(answer: Option<&[u8]>) -> String {
    asked_after(answer, Duration::ZERO)
}

#[test]
fn late_answer_read() {
    // Past the background query's own wait. The query ends on the status reply,
    // which this answer carries last.
    let text = asked_after(
        Some(b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[0n"),
        Duration::from_millis(250),
    );
    assert!(
        text.contains("got z"),
        "a late answer reached the reader as keys: {text}"
    );
    let line = asked_line(&text);
    assert!(line.contains("Kitty"), "a late answer was not read: {line}");
}

#[test]
fn key_during_silent_query() {
    // A key and no answer, partway into the wait of a terminal silent to the
    // query. The key is the query's to drop; the one after it is the reader's.
    let text = asked_after(Some(b"k"), vigia::PICTURES_WAIT / 3);
    assert!(
        text.contains("got z"),
        "the key after the query was lost: {text}"
    );
    let line = asked_line(&text);
    let waited = pty::waited_ms(&text);
    assert!(
        line.contains("Halfblocks") && waited < vigia::PICTURES_WAIT.as_secs_f64() * 1000.0 + 200.0,
        "a key typed into a silent query stalled it or chose a protocol: {line}"
    );
}

#[test]
fn piped_picker_silent() {
    // `/dev/tty` is all the query could use, and on macOS it cannot read it.
    let mut child = Command::new(std::env::current_exe().expect("this test"));
    child
        .args(["--exact", "silent_child", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env("TERM", "xterm-256color")
        .env_remove("TERM_PROGRAM")
        .env_remove(vigia::pictures::PICTURES_VAR);
    let text = pty::run(
        child,
        pty::Script {
            marker: "[5n",
            answer: None,
            delay: Duration::ZERO,
            piped: true,
            detached: false,
            quiet: false,
        },
    );
    assert!(
        text.contains("got z"),
        "the key after the query was lost: {text}"
    );
    assert!(
        asked_line(&text).contains("Halfblocks"),
        "a piped stdin chose a protocol: {text}"
    );
    if cfg!(target_os = "macos") {
        assert!(
            !text.contains("_Gi=31"),
            "the picture query was sent where its answer cannot be read: {text}"
        );
    }
}

/// [`asked_in_pty`] with the answer sent `delay` after the query arrives.
fn asked_after(answer: Option<&[u8]>, delay: Duration) -> String {
    let mut child = Command::new(std::env::current_exe().expect("this test"));
    child
        .args(["--exact", "silent_child", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .env("TERM", "xterm-256color")
        .env_remove("TERM_PROGRAM")
        .env_remove(vigia::pictures::PICTURES_VAR);
    pty::run(
        child,
        pty::Script {
            marker: "[5n",
            answer,
            delay,
            piped: false,
            detached: false,
            quiet: false,
        },
    )
}
