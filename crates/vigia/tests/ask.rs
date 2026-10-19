//! The background query against terminals that answer, do not, or stop
//! halfway. `SPEC.md` §11.1.
#![cfg(unix)]

#[path = "pty/mod.rs"]
mod pty;

use std::io::{Read, Write};
use std::process::Command;
use std::time::{Duration, Instant};

/// Set in the child this test starts inside a fresh terminal.
const CHILD: &str = "VIGIA_ASK_CHILD";
/// Set in the child when it asks for the palette too.
const PALETTE: &str = "VIGIA_ASK_PALETTE";
/// The background query's wait, as `run` passes it.
const WAIT: Duration = Duration::from_millis(150);
/// How long a palette answer that has begun is given to finish.
const ANSWERING: Duration = Duration::from_secs(1);
/// Room for a loaded machine past a bound.
const SLACK: Duration = Duration::from_millis(200);

/// The child's half: ask, say how long it took and what came back, then read
/// one byte from the terminal, which it gets only if nothing else is reading.
#[test]
fn ask_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let began = Instant::now();
    let reply = vigia::ask(WAIT, std::env::var_os(PALETTE).is_some());
    let waited = began.elapsed();
    println!(
        "asked {:.3} {}",
        waited.as_secs_f64() * 1000.0,
        String::from_utf8_lossy(&reply).escape_debug()
    );
    std::io::stdout().flush().expect("flush");
    let mut tty = std::fs::File::open("/dev/tty").expect("the child has a terminal");
    let mut byte = [0u8; 1];
    tty.read_exact(&mut byte).expect("read the byte");
    println!("got {}", byte[0] as char);
}

#[test]
fn silent_ask_bounded() {
    let text = asked(Ask::default());
    assert_typed(&text);
    assert_within(&text, WAIT);
}

#[test]
fn answered_ask() {
    let text = asked(Ask {
        answer: Some(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
        ..Ask::default()
    });
    assert_typed(&text);
    assert!(
        pty::asked_line(&text).contains("]11;rgb:"),
        "the answer was not read: {text}"
    );
}

#[test]
fn piped_stdin_ask() {
    let text = asked(Ask {
        piped: true,
        ..Ask::default()
    });
    assert_typed(&text);
    assert_within(&text, WAIT);
}

#[test]
fn partial_palette_ask() {
    // The background and no more: the palette has begun and never ends.
    let text = asked(Ask {
        answer: Some(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
        palette: true,
        ..Ask::default()
    });
    assert_typed(&text);
    assert_within(&text, ANSWERING);
}

/// How the terminal behaves toward one child.
#[derive(Default)]
struct Ask {
    /// Sent once the query arrives, or nothing.
    answer: Option<&'static [u8]>,
    /// The child asks for the palette too.
    palette: bool,
    /// The child's stdin is not the terminal.
    piped: bool,
}

fn assert_typed(text: &str) {
    assert!(
        text.contains("got z"),
        "the key after the query never reached the reader: {text}"
    );
}

fn assert_within(text: &str, bound: Duration) {
    let waited = pty::waited_ms(text);
    assert!(
        waited < (bound + SLACK).as_secs_f64() * 1000.0,
        "the query waited {waited}ms, past its {bound:?}: {}",
        pty::asked_line(text)
    );
}

/// Run the child in a fresh terminal behaving as `ask` says.
fn asked(ask: Ask) -> String {
    let mut child = Command::new(std::env::current_exe().expect("this test"));
    child
        .args(["--exact", "ask_child", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1");
    if ask.palette {
        child.env(PALETTE, "1");
    }
    pty::run(
        child,
        pty::Script {
            marker: "]11;?",
            answer: ask.answer,
            late: Duration::ZERO,
            piped: ask.piped,
        },
    )
}
